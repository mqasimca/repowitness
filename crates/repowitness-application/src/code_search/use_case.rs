/// Runs one bounded search and maps storage-neutral candidates to attributed evidence.
pub fn code_search<Port>(
    port: &Port,
    request: CodeSearchRequest,
) -> Result<CodeSearchResult<Port::Generation>, CodeSearchError<Port::Error>>
where
    Port: CodeSearchPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let query_digest = request.query.digest();
    let limits = request.limits;
    let repository = request.repository;
    let result = port
        .search(
            repository,
            &request.query,
            limits,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(CodeSearchError::Port)?;
    check_control(&request.cancelled, request.deadline)?;

    let (returned_matches, omitted_matches) = validate_port_result(&result, limits)?;
    let coverage = search_coverage(result.index_coverage, returned_matches, omitted_matches)?;
    let evidence = search_evidence(repository, result.snapshot, result.candidates, limits)?;
    let notices = search_notices()?;
    let resolution = if returned_matches == 0 {
        ResolutionStatus::Unresolved
    } else {
        ResolutionStatus::Confirmed
    };
    MaterialResult::try_new(
        CodeSearchClaim {
            query: query_digest,
            returned_matches,
            total_matches: result.total_matches,
        },
        evidence,
        resolution,
        result.snapshot,
        result.generation,
        notices,
        coverage,
    )
    .map_err(CodeSearchError::MaterialResult)
}

fn validate_port_result<G, E>(
    result: &CodeSearchPortResult<G>,
    limits: CodeSearchLimits,
) -> Result<(u64, u64), CodeSearchError<E>> {
    let returned_matches = u64::try_from(result.candidates.len()).map_err(|_| {
        CodeSearchError::InvalidPortOutput(CodeSearchPortOutputError::CountNotRepresentable)
    })?;
    if returned_matches > u64::from(limits.max_results()) {
        return Err(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::CandidateLimitExceeded,
        ));
    }
    let omitted_matches = result.total_matches.checked_sub(returned_matches).ok_or(
        CodeSearchError::InvalidPortOutput(CodeSearchPortOutputError::InvalidTotalMatches),
    )?;
    if result.output_bytes > limits.max_output_bytes() {
        return Err(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::OutputByteLimitExceeded,
        ));
    }
    if result.candidates.iter().any(|candidate| {
        !candidate
            .occurrence
            .language()
            .matches_repository_path(&candidate.path)
    }) {
        return Err(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::InvalidCandidate,
        ));
    }
    Ok((returned_matches, omitted_matches))
}

fn search_coverage<E>(
    index: RustIndexCoverage,
    returned_matches: u64,
    omitted_matches: u64,
) -> Result<CoverageSummary, CodeSearchError<E>> {
    let unresolved = index
        .unresolved()
        .checked_add(u64::from(returned_matches == 0))
        .ok_or(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::CountNotRepresentable,
        ))?;
    let truncated = index.truncated().checked_add(omitted_matches).ok_or(
        CodeSearchError::InvalidPortOutput(CodeSearchPortOutputError::CountNotRepresentable),
    )?;
    Ok(CoverageSummary::new(
        CoverageItemCount::new(index.searched()),
        CoverageItemCount::new(index.skipped()),
        CoverageItemCount::new(unresolved),
        CoverageItemCount::new(truncated),
    ))
}

fn search_evidence<E>(
    repository: RepositoryIdentityDigest,
    snapshot: SourceSnapshotDigest,
    candidates: Vec<CodeSearchCandidate>,
    limits: CodeSearchLimits,
) -> Result<
    BoundedResultItems<EvidenceRecord<CodeSearchEvidenceIdentity, CodeSearchProducerIdentity>>,
    CodeSearchError<E>,
> {
    let evidence = candidates
        .into_iter()
        .map(|candidate| {
            let (path, content_digest, occurrence) = candidate.into_parts();
            let producer = ProducerIdentity::new(
                match occurrence.language() {
                    SourceLanguage::Rust => CodeSearchProducer::RustSyntax,
                    SourceLanguage::Go => CodeSearchProducer::GoSyntax,
                    SourceLanguage::TypeScript => CodeSearchProducer::TypeScriptSyntax,
                    SourceLanguage::Tsx => CodeSearchProducer::TsxSyntax,
                    SourceLanguage::Python => CodeSearchProducer::PythonSyntax,
                },
                occurrence.producer_manifest(),
            );
            EvidenceRecord::new(
                EvidenceIdentity::new(
                    repository,
                    snapshot,
                    path,
                    content_digest,
                    EvidenceLocation::SymbolOccurrence(occurrence),
                ),
                producer,
                EvidenceTier::Syntax,
                EvidenceRelation::Supports,
            )
        })
        .collect();
    BoundedResultItems::try_from_vec(
        evidence,
        ResultItemLimit::new(u64::from(limits.max_results())),
    )
    .map_err(CodeSearchError::ResultItems)
}

fn search_notices<E>()
-> Result<BoundedResultItems<ResultNotice<CodeSearchNotice>>, CodeSearchError<E>> {
    BoundedResultItems::try_from_vec(
        vec![ResultNotice::new(
            ResultNoticeKind::Limitation,
            CodeSearchNotice::SupportedLanguageSymbolLexicalOnly,
        )],
        ResultItemLimit::new(1),
    )
    .map_err(CodeSearchError::ResultItems)
}

fn check_control<E>(cancelled: &AtomicBool, deadline: Instant) -> Result<(), CodeSearchError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(CodeSearchError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(CodeSearchError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
