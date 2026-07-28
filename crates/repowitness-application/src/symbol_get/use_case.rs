/// Retrieves one exact active-generation occurrence with attributed evidence.
pub fn symbol_get<Port>(
    port: &Port,
    request: SymbolGetRequest<Port::Generation>,
) -> Result<SymbolGetResult<Port::Generation>, SymbolGetError<Port::Error>>
where
    Port: SymbolGetPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let result = port
        .get(SymbolGetPortRequest {
            repository: request.repository,
            expected_snapshot: request.expected_snapshot,
            expected_generation: request.expected_generation,
            selector: request.selector.clone(),
            limits: request.limits,
            cancelled: Arc::clone(&request.cancelled),
            deadline: request.deadline,
        })
        .map_err(SymbolGetError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_context(&request, &result)?;
    let candidate = validate_candidate(&request.selector, result.candidate, request.limits)?;
    let coverage = symbol_coverage(result.index_coverage, candidate.is_none())?;
    let (symbol, evidence) = symbol_and_evidence(request.repository, result.snapshot, candidate)?;
    let resolution = if symbol.is_some() {
        ResolutionStatus::Confirmed
    } else {
        ResolutionStatus::Unresolved
    };
    MaterialResult::try_new(
        SymbolGetClaim {
            selector: request.selector,
            symbol,
        },
        evidence,
        resolution,
        result.snapshot,
        result.generation,
        symbol_notices()?,
        coverage,
    )
    .map_err(SymbolGetError::MaterialResult)
}

fn validate_context<G: Eq, E>(
    request: &SymbolGetRequest<G>,
    result: &SymbolGetPortResult<G>,
) -> Result<(), SymbolGetError<E>> {
    if result.snapshot != request.expected_snapshot
        || result.generation != request.expected_generation
    {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::ContextMismatch,
        ));
    }
    Ok(())
}

fn validate_candidate<E>(
    selector: &SymbolGetSelector,
    candidate: Option<SymbolGetCandidate>,
    limits: SymbolGetLimits,
) -> Result<Option<SymbolGetCandidate>, SymbolGetError<E>> {
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    if candidate.path != selector.path
        || candidate.content_digest != selector.content_digest
        || candidate.occurrence.artifact_digest() != selector.artifact_digest
        || candidate.occurrence.fact_ordinal() != selector.fact_ordinal
    {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::SelectorMismatch,
        ));
    }
    if !candidate
        .occurrence
        .language()
        .matches_repository_path(&candidate.path)
    {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::LanguagePathMismatch,
        ));
    }
    validate_declaration(&candidate, limits)?;
    Ok(Some(candidate))
}

fn validate_declaration<E>(
    candidate: &SymbolGetCandidate,
    limits: SymbolGetLimits,
) -> Result<(), SymbolGetError<E>> {
    let declaration_bytes = fixed_count(candidate.declaration.len())?;
    if declaration_bytes > limits.max_declaration_bytes() {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::DeclarationLimitExceeded,
        ));
    }
    let occurrence = &candidate.occurrence;
    if declaration_bytes != occurrence.declaration_span().len().get()
        || !declaration_contains_exact_name(occurrence, &candidate.declaration)
    {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::InvalidDeclaration,
        ));
    }
    let output_bytes = FIXED_OCCURRENCE_OUTPUT_BYTES
        .checked_add(fixed_count(candidate.path.as_bytes().len())?)
        .and_then(|bytes| bytes.checked_add(declaration_bytes))
        .and_then(|bytes| bytes.checked_add(u64::try_from(occurrence.name().len()).ok()?))
        .and_then(|bytes| bytes.checked_add(u64::try_from(occurrence.qualified_name().len()).ok()?))
        .ok_or(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::CountNotRepresentable,
        ))?;
    if output_bytes > limits.max_output_bytes() {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::OutputByteLimitExceeded,
        ));
    }
    Ok(())
}

fn declaration_contains_exact_name(occurrence: &RustSymbolOccurrence, declaration: &[u8]) -> bool {
    let declaration_start = occurrence.declaration_span().start().get();
    let Some(relative_start) = occurrence
        .name_span()
        .start()
        .get()
        .checked_sub(declaration_start)
    else {
        return false;
    };
    let Some(relative_end) = occurrence
        .name_span()
        .end()
        .get()
        .checked_sub(declaration_start)
    else {
        return false;
    };
    let (Ok(relative_start), Ok(relative_end)) = (
        usize::try_from(relative_start),
        usize::try_from(relative_end),
    ) else {
        return false;
    };
    declaration.get(relative_start..relative_end) == Some(occurrence.name().as_bytes())
}

fn fixed_count<E>(count: usize) -> Result<u64, SymbolGetError<E>> {
    u64::try_from(count).map_err(|_| {
        SymbolGetError::InvalidPortOutput(SymbolGetPortOutputError::CountNotRepresentable)
    })
}

fn symbol_coverage<E>(
    index: RustIndexCoverage,
    missing: bool,
) -> Result<CoverageSummary, SymbolGetError<E>> {
    let unresolved = index.unresolved().checked_add(u64::from(missing)).ok_or(
        SymbolGetError::InvalidPortOutput(SymbolGetPortOutputError::CountNotRepresentable),
    )?;
    Ok(CoverageSummary::new(
        CoverageItemCount::new(index.searched()),
        CoverageItemCount::new(index.skipped()),
        CoverageItemCount::new(unresolved),
        CoverageItemCount::new(index.truncated()),
    ))
}

type SymbolEvidence =
    BoundedResultItems<EvidenceRecord<SymbolGetEvidenceIdentity, SymbolGetProducerIdentity>>;
type SymbolAndEvidence = (Option<RetrievedSymbol>, SymbolEvidence);

fn symbol_and_evidence<E>(
    repository: RepositoryIdentityDigest,
    snapshot: SourceSnapshotDigest,
    candidate: Option<SymbolGetCandidate>,
) -> Result<SymbolAndEvidence, SymbolGetError<E>> {
    let Some(candidate) = candidate else {
        let evidence = BoundedResultItems::try_from_vec(Vec::new(), ResultItemLimit::new(1))
            .map_err(SymbolGetError::ResultItems)?;
        return Ok((None, evidence));
    };
    let evidence_occurrence = candidate.occurrence.clone();
    let producer = match evidence_occurrence.language() {
        SourceLanguage::Rust => SymbolGetProducer::RustSyntax,
        SourceLanguage::Go => SymbolGetProducer::GoSyntax,
        SourceLanguage::TypeScript => SymbolGetProducer::TypeScriptSyntax,
        SourceLanguage::Tsx => SymbolGetProducer::TsxSyntax,
        SourceLanguage::Python => SymbolGetProducer::PythonSyntax,
    };
    let producer_manifest = evidence_occurrence.producer_manifest();
    let evidence = EvidenceRecord::new(
        EvidenceIdentity::new(
            repository,
            snapshot,
            candidate.path,
            candidate.content_digest,
            EvidenceLocation::SymbolOccurrence(evidence_occurrence),
        ),
        ProducerIdentity::new(producer, producer_manifest),
        EvidenceTier::Syntax,
        EvidenceRelation::Supports,
    );
    let evidence = BoundedResultItems::try_from_vec(vec![evidence], ResultItemLimit::new(1))
        .map_err(SymbolGetError::ResultItems)?;
    Ok((
        Some(RetrievedSymbol {
            occurrence: candidate.occurrence,
            declaration: candidate.declaration,
        }),
        evidence,
    ))
}

fn symbol_notices<E>()
-> Result<BoundedResultItems<ResultNotice<SymbolGetNotice>>, SymbolGetError<E>> {
    BoundedResultItems::try_from_vec(
        vec![ResultNotice::new(
            ResultNoticeKind::Limitation,
            SymbolGetNotice::DefinitionOnlyNoReferences,
        )],
        ResultItemLimit::new(1),
    )
    .map_err(SymbolGetError::ResultItems)
}

fn check_control<E>(cancelled: &AtomicBool, deadline: Instant) -> Result<(), SymbolGetError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(SymbolGetError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SymbolGetError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
