use repowitness_local::BoundedFileReadError;

fn read_bounded_regular_file(
    path: &Path,
    maximum_bytes: usize,
) -> Result<Vec<u8>, BoundedFileReadError> {
    repowitness_local::read_bounded_regular_file(path, maximum_bytes)
        .map(|contents| contents.into_bytes().into_vec())
}
