use std::{
    error::Error,
    fmt, io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, ReadBuf};

/// Inclusive maximum bytes in one MCP JSON-RPC input line, excluding newline.
pub const MAX_MCP_INPUT_LINE_BYTES: usize = 3 * 1024 * 1024;

const READ_CHUNK_BYTES: usize = 8 * 1024;

/// Stable failure to construct a bounded line reader.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedLineReaderLimitError;

impl fmt::Display for BoundedLineReaderLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MCP input-line byte limit must be positive")
    }
}

impl Error for BoundedLineReaderLimitError {}

/// Async reader that fails closed when any newline-delimited message is too large.
///
/// The wrapper limits every underlying read to the remaining line budget plus
/// one byte, so the SDK's line buffer cannot grow beyond the declared ceiling.
pub struct BoundedLineReader<R> {
    inner: R,
    max_line_bytes: usize,
    current_line_bytes: usize,
    failed: bool,
}

impl<R> BoundedLineReader<R> {
    /// Wraps an async reader with an inclusive positive line bound.
    pub fn try_new(inner: R, max_line_bytes: usize) -> Result<Self, BoundedLineReaderLimitError> {
        if max_line_bytes == 0 {
            return Err(BoundedLineReaderLimitError);
        }
        Ok(Self {
            inner,
            max_line_bytes,
            current_line_bytes: 0,
            failed: false,
        })
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for BoundedLineReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.failed {
            return Poll::Ready(Err(line_too_large()));
        }
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        let remaining_line = self.max_line_bytes - self.current_line_bytes;
        let read_bytes = output
            .remaining()
            .min(remaining_line.saturating_add(1))
            .min(READ_CHUNK_BYTES);
        let mut scratch = [0_u8; READ_CHUNK_BYTES];
        let mut input = ReadBuf::new(&mut scratch[..read_bytes]);
        match Pin::new(&mut self.inner).poll_read(context, &mut input) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Ready(Ok(())) => {
                let bytes = input.filled();
                for byte in bytes {
                    if *byte == b'\n' {
                        self.current_line_bytes = 0;
                    } else {
                        self.current_line_bytes += 1;
                        if self.current_line_bytes > self.max_line_bytes {
                            self.failed = true;
                            return Poll::Ready(Err(line_too_large()));
                        }
                    }
                }
                output.put_slice(bytes);
                Poll::Ready(Ok(()))
            }
        }
    }
}

fn line_too_large() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "MCP input line exceeds byte limit",
    )
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt;

    use super::*;

    #[tokio::test]
    async fn accepts_exact_limit_and_resets_at_each_newline() {
        let input = b"1234\nabcd\n";
        let mut reader = BoundedLineReader::try_new(input.as_slice(), 4).expect("positive limit");
        let mut output = Vec::new();
        reader
            .read_to_end(&mut output)
            .await
            .expect("both exact lines fit");
        assert_eq!(output, input);
    }

    #[tokio::test]
    async fn rejects_one_extra_byte_and_remains_failed() {
        let mut reader =
            BoundedLineReader::try_new(b"12345\n".as_slice(), 4).expect("positive limit");
        let mut output = Vec::new();
        let error = reader
            .read_to_end(&mut output)
            .await
            .expect_err("oversized line must fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(output.is_empty());

        let mut byte = [0_u8; 1];
        assert_eq!(
            reader
                .read(&mut byte)
                .await
                .expect_err("reader remains failed")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn zero_limit_is_rejected() {
        assert!(BoundedLineReader::try_new(&b""[..], 0).is_err());
    }
}
