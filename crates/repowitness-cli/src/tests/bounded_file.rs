use super::*;

struct BoundedFileTempDirectory(PathBuf);

impl BoundedFileTempDirectory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let ordinal = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let physical_temporary_directory = std::fs::canonicalize(std::env::temp_dir())
            .expect("canonicalize temporary directory for no-follow fixture");
        let path = physical_temporary_directory.join(format!(
            "repowitness-cli-bounded-file-{}-{ordinal}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create bounded-file fixture directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for BoundedFileTempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cli_bounded_reader_preserves_bytes_and_maps_size_errors() {
    let directory = BoundedFileTempDirectory::new();
    let path = directory.path().join("--config");
    std::fs::write(&path, b"safe").expect("write bounded fixture");

    assert_eq!(
        read_bounded_regular_file(&path, 4).expect("read exact limit"),
        b"safe"
    );
    assert_eq!(
        read_bounded_regular_file(&path, 3).expect_err("reject one byte over"),
        BoundedFileReadError::TooLarge
    );
}
