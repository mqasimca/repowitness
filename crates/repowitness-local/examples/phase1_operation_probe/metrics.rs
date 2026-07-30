use std::{
    error::Error,
    ffi::OsString,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::Duration,
};

type MetricResult<T> = Result<T, Box<dyn Error>>;

pub(super) fn hex_digest(bytes: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

pub(super) fn nearest_rank_p95(samples: &mut [Duration]) -> MetricResult<Duration> {
    if samples.len() < 2 {
        return Err("at least two timing samples are required".into());
    }
    samples.sort_unstable();
    let rank = samples
        .len()
        .checked_mul(95)
        .and_then(|value| value.checked_add(99))
        .ok_or("timing rank overflowed")?
        / 100;
    samples
        .get(rank.saturating_sub(1))
        .copied()
        .ok_or_else(|| "timing percentile was unavailable".into())
}

pub(super) fn required_file_size(path: &Path) -> MetricResult<u64> {
    Ok(fs::metadata(path)?.len())
}

pub(super) fn wal_file_size(database: &Path) -> MetricResult<u64> {
    let mut path = PathBuf::from(database);
    let mut name = path.file_name().map_or_else(OsString::new, OsString::from);
    name.push("-wal");
    path.set_file_name(name);
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{hex_digest, nearest_rank_p95};

    #[test]
    fn digest_hex_is_lowercase_and_fixed_width() {
        assert_eq!(hex_digest(&[0xAB; 32]), "ab".repeat(32));
    }

    #[test]
    fn nearest_rank_is_deterministic_and_inclusive() {
        let mut two = [Duration::from_millis(9), Duration::from_millis(1)];
        assert_eq!(
            nearest_rank_p95(&mut two).expect("p95"),
            Duration::from_millis(9)
        );
        let mut hundred = (1_u64..=100)
            .rev()
            .map(Duration::from_millis)
            .collect::<Vec<_>>();
        assert_eq!(
            nearest_rank_p95(&mut hundred).expect("p95"),
            Duration::from_millis(95)
        );
    }

    #[test]
    fn percentile_rejects_an_underfilled_sample() {
        assert!(nearest_rank_p95(&mut [Duration::ZERO]).is_err());
    }
}
