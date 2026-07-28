fn parse_index_scope(
    output: &[u8],
    object_format: GitObjectFormat,
    limits: GitPathDiscoveryLimits,
) -> Result<(), SourceStateError> {
    let records = nul_records(output).ok_or(SourceStateError::InvalidIndexRecord)?;
    let mut paths = BTreeMap::<RepositoryPath, u8>::new();
    for record in records {
        let (metadata, path) = split_once(record, b'\t')
            .filter(|(_, path)| !path.is_empty())
            .ok_or(SourceStateError::InvalidIndexRecord)?;
        let fields = metadata.split(|byte| *byte == b' ').collect::<Vec<_>>();
        if fields.len() != 4
            || !valid_index_tag(fields[0])
            || parse_mode(fields[1]).is_none()
            || decode_object_id(fields[2], object_format).is_none()
            || !matches!(fields[3], b"0" | b"1" | b"2" | b"3")
        {
            return Err(SourceStateError::InvalidIndexRecord);
        }
        if matches!(fields[0], b"S" | b"s") {
            return Err(SourceStateError::SparseWorktreeUnsupported);
        }
        if fields[1] == b"040000" {
            return Err(SourceStateError::SparseWorktreeUnsupported);
        }
        if fields[1] == b"160000" {
            return Err(SourceStateError::SubmoduleUnsupported);
        }
        let stage = fields[3][0] - b'0';
        let ordinal = u64::try_from(paths.len())
            .ok()
            .and_then(|count| count.checked_add(1))
            .ok_or(SourceStateError::RecordCountNotRepresentable)?;
        let path = validate_path(path, ordinal, limits)?;
        if !paths.contains_key(&path)
            && u64::try_from(paths.len()).unwrap_or(u64::MAX) >= limits.paths()
        {
            return Err(SourceStateError::StatusPathLimitExceeded {
                limit: limits.paths(),
            });
        }
        match paths.entry(path) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(1_u8 << stage);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let existing = *entry.get();
                let stage_mask = 1_u8 << stage;
                if existing & stage_mask != 0 || stage == 0 || existing & 1 != 0 {
                    return Err(SourceStateError::InvalidIndexRecord);
                }
                *entry.get_mut() = existing | stage_mask;
            }
        }
    }
    Ok(())
}

fn parse_status_records(
    output: &[u8],
    object_format: GitObjectFormat,
    limits: GitPathDiscoveryLimits,
) -> Result<Box<[CanonicalStatusRecord]>, SourceStateError> {
    let records = nul_records(output).ok_or(SourceStateError::InvalidStatusRecord)?;
    let mut parsed = Vec::new();
    let mut count = 0_u64;
    for record in records {
        count = checked_record_count(count, limits)?;
        let parsed_record = match record.first() {
            Some(b'1') => parse_ordinary_record(record, object_format, count, limits)?,
            Some(b'u') => parse_unmerged_record(record, object_format, count, limits)?,
            Some(b'?') => parse_untracked_record(record, count, limits)?,
            Some(b'2') => return Err(SourceStateError::InvalidStatusRecord),
            Some(b'#' | b'!' | b'S') => return Err(SourceStateError::InvalidStatusRecord),
            _ => return Err(SourceStateError::InvalidStatusRecord),
        };
        parsed.push(parsed_record);
    }
    parsed.sort_unstable_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.tag.cmp(&right.tag))
    });
    if parsed
        .windows(2)
        .any(|records| records[0].path == records[1].path)
    {
        return Err(SourceStateError::DuplicateStatusPath);
    }
    Ok(parsed.into_boxed_slice())
}

fn parse_ordinary_record(
    record: &[u8],
    object_format: GitObjectFormat,
    ordinal: u64,
    limits: GitPathDiscoveryLimits,
) -> Result<CanonicalStatusRecord, SourceStateError> {
    let fields = record.splitn(9, |byte| *byte == b' ').collect::<Vec<_>>();
    if fields.len() != 9
        || fields[0] != b"1"
        || !valid_ordinary_status(fields[1])
        || !valid_non_submodule_field(fields[2])?
    {
        return Err(SourceStateError::InvalidStatusRecord);
    }
    let mut canonical_fields = Vec::with_capacity(96);
    canonical_fields.extend_from_slice(fields[1]);
    canonical_fields.extend_from_slice(fields[2]);
    for mode in &fields[3..6] {
        append_mode(&mut canonical_fields, mode)?;
    }
    for object_id in &fields[6..8] {
        append_object_id(&mut canonical_fields, object_id, object_format)?;
    }
    let path = validate_path(fields[8], ordinal, limits)?;
    Ok(CanonicalStatusRecord {
        path,
        tag: 1,
        fields: canonical_fields.into_boxed_slice(),
    })
}

fn parse_unmerged_record(
    record: &[u8],
    object_format: GitObjectFormat,
    ordinal: u64,
    limits: GitPathDiscoveryLimits,
) -> Result<CanonicalStatusRecord, SourceStateError> {
    let fields = record.splitn(11, |byte| *byte == b' ').collect::<Vec<_>>();
    if fields.len() != 11
        || fields[0] != b"u"
        || !valid_unmerged_status(fields[1])
        || !valid_non_submodule_field(fields[2])?
    {
        return Err(SourceStateError::InvalidStatusRecord);
    }
    let mut canonical_fields = Vec::with_capacity(132);
    canonical_fields.extend_from_slice(fields[1]);
    canonical_fields.extend_from_slice(fields[2]);
    for mode in &fields[3..7] {
        append_mode(&mut canonical_fields, mode)?;
    }
    for object_id in &fields[7..10] {
        append_object_id(&mut canonical_fields, object_id, object_format)?;
    }
    let path = validate_path(fields[10], ordinal, limits)?;
    Ok(CanonicalStatusRecord {
        path,
        tag: 2,
        fields: canonical_fields.into_boxed_slice(),
    })
}

fn parse_untracked_record(
    record: &[u8],
    ordinal: u64,
    limits: GitPathDiscoveryLimits,
) -> Result<CanonicalStatusRecord, SourceStateError> {
    let path = record
        .strip_prefix(b"? ")
        .filter(|path| !path.is_empty())
        .ok_or(SourceStateError::InvalidStatusRecord)?;
    Ok(CanonicalStatusRecord {
        path: validate_path(path, ordinal, limits)?,
        tag: 3,
        fields: Box::new([]),
    })
}

fn valid_ordinary_status(status: &[u8]) -> bool {
    status.len() == 2
        && matches!(status[0], b'.' | b'M' | b'T' | b'A' | b'D' | b'R' | b'C')
        && matches!(status[1], b'.' | b'M' | b'T' | b'D' | b'R' | b'C')
        && status != b".."
}

fn valid_unmerged_status(status: &[u8]) -> bool {
    matches!(
        status,
        b"DD" | b"AU" | b"UD" | b"UA" | b"DU" | b"AA" | b"UU"
    )
}

fn valid_non_submodule_field(field: &[u8]) -> Result<bool, SourceStateError> {
    if field.len() != 4 {
        return Ok(false);
    }
    if field[0] == b'S' {
        return Err(SourceStateError::SubmoduleUnsupported);
    }
    Ok(field == b"N...")
}

fn append_mode(target: &mut Vec<u8>, field: &[u8]) -> Result<(), SourceStateError> {
    let mode = parse_mode(field).ok_or(SourceStateError::InvalidStatusRecord)?;
    if field == b"040000" {
        return Err(SourceStateError::SparseWorktreeUnsupported);
    }
    if field == b"160000" {
        return Err(SourceStateError::SubmoduleUnsupported);
    }
    target.extend_from_slice(&mode.to_be_bytes());
    Ok(())
}

fn parse_mode(field: &[u8]) -> Option<u32> {
    if !matches!(
        field,
        b"000000" | b"040000" | b"100644" | b"100755" | b"120000" | b"160000"
    ) {
        return None;
    }
    field.iter().try_fold(0_u32, |mode, byte| {
        mode.checked_mul(8)?.checked_add(u32::from(*byte - b'0'))
    })
}

fn valid_index_tag(field: &[u8]) -> bool {
    matches!(
        field,
        b"H" | b"h" | b"S" | b"s" | b"M" | b"m" | b"R" | b"r" | b"C" | b"c" | b"K" | b"k"
    )
}

fn append_object_id(
    target: &mut Vec<u8>,
    field: &[u8],
    object_format: GitObjectFormat,
) -> Result<(), SourceStateError> {
    let object_id =
        decode_object_id(field, object_format).ok_or(SourceStateError::InvalidStatusRecord)?;
    target.push(object_format.tag());
    target.push(u8::try_from(object_id.len()).map_err(|_| SourceStateError::InvalidStatusRecord)?);
    target.extend_from_slice(&object_id);
    Ok(())
}

fn decode_object_id(field: &[u8], object_format: GitObjectFormat) -> Option<Box<[u8]>> {
    if field.len() != object_format.object_id_bytes().checked_mul(2)?
        || !field
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return None;
    }
    let decoded = field
        .chunks_exact(2)
        .map(|pair| decode_nibble(pair[0]).zip(decode_nibble(pair[1])))
        .map(|pair| pair.map(|(high, low)| (high << 4) | low))
        .collect::<Option<Vec<_>>>()?;
    Some(decoded.into_boxed_slice())
}

const fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn validate_path(
    path: &[u8],
    ordinal: u64,
    limits: GitPathDiscoveryLimits,
) -> Result<RepositoryPath, SourceStateError> {
    RepositoryPath::try_from_bytes(path, limits.repository_path())
        .map_err(|source| SourceStateError::InvalidRepositoryPath { ordinal, source })
}

fn checked_record_count(
    count: u64,
    limits: GitPathDiscoveryLimits,
) -> Result<u64, SourceStateError> {
    let next = count
        .checked_add(1)
        .ok_or(SourceStateError::RecordCountNotRepresentable)?;
    if next > limits.paths() {
        return Err(SourceStateError::StatusPathLimitExceeded {
            limit: limits.paths(),
        });
    }
    Ok(next)
}

struct NulRecords<'a> {
    remaining: Option<&'a [u8]>,
}

impl<'a> Iterator for NulRecords<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let remaining = self.remaining.take()?;
        if remaining.is_empty() {
            return None;
        }
        match remaining.iter().position(|byte| *byte == 0) {
            Some(position) => {
                self.remaining = Some(&remaining[position + 1..]);
                Some(&remaining[..position])
            }
            None => Some(remaining),
        }
    }
}

fn nul_records(output: &[u8]) -> Option<NulRecords<'_>> {
    if output.is_empty() {
        return Some(NulRecords { remaining: None });
    }
    let records = output.strip_suffix(b"\0")?;
    if records.split(|byte| *byte == 0).any(<[u8]>::is_empty) {
        return None;
    }
    Some(NulRecords {
        remaining: Some(records),
    })
}

fn split_once(bytes: &[u8], delimiter: u8) -> Option<(&[u8], &[u8])> {
    let position = bytes.iter().position(|byte| *byte == delimiter)?;
    Some((&bytes[..position], &bytes[position + 1..]))
}

fn hash_git_state(
    object_format: GitObjectFormat,
    head: Option<&[u8]>,
    shallow: bool,
) -> GitStateDigest {
    let mut hasher = Sha256::new();
    hasher.update(GIT_STATE_DOMAIN);
    hasher.update(GIT_STATE_VERSION.to_be_bytes());
    hasher.update([object_format.tag()]);
    match head {
        Some(object_id) => {
            hasher.update([1]);
            hasher.update([u8::try_from(object_id.len()).unwrap_or(u8::MAX)]);
            hasher.update(object_id);
        }
        None => hasher.update([0, 0]),
    }
    hasher.update([u8::from(shallow)]);
    GitStateDigest::new(hasher.finalize().into())
}

fn hash_worktree_state(
    records: &[CanonicalStatusRecord],
    manifest: SourceManifestDigest,
) -> WorktreeStateDigest {
    hash_worktree_state_with_profile(
        records,
        manifest,
        RUST_WORKTREE_STATE_DOMAIN,
        RUST_WORKTREE_STATE_VERSION,
    )
}

fn hash_worktree_state_with_profile(
    records: &[CanonicalStatusRecord],
    manifest: SourceManifestDigest,
    domain: &[u8],
    version: u32,
) -> WorktreeStateDigest {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(version.to_be_bytes());
    hasher.update(GIT_STATUS_PROFILE_VERSION.to_be_bytes());
    hasher.update(
        u64::try_from(records.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for record in records {
        hasher.update([record.tag]);
        hasher.update(
            u64::try_from(record.fields.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(&record.fields);
        hasher.update(
            u64::try_from(record.path.as_bytes().len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(record.path.as_bytes());
    }
    hasher.update(manifest.as_bytes());
    WorktreeStateDigest::new(hasher.finalize().into())
}
