use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use repowitness_domain::EvidenceLocation;
use repowitness_local::{LocalCodeSearchRequest, search_local_index};

use crate::ProbeResult;

const SEARCH_RESULTS: u16 = 100;

struct RequiredEvidence {
    query: &'static str,
    path: &'static [u8],
    name: &'static str,
}

const REQUIRED_EVIDENCE: [RequiredEvidence; 9] = [
    RequiredEvidence {
        query: "check",
        path: b"src/frame.rs",
        name: "check",
    },
    RequiredEvidence {
        query: "check_rejects_invalid_negative_bulk_length",
        path: b"tests/frame_validation.rs",
        name: "check_rejects_invalid_negative_bulk_length",
    },
    RequiredEvidence {
        query: "run",
        path: b"src/server.rs",
        name: "run",
    },
    RequiredEvidence {
        query: "Listener",
        path: b"src/server.rs",
        name: "Listener",
    },
    RequiredEvidence {
        query: "Handler",
        path: b"src/server.rs",
        name: "Handler",
    },
    RequiredEvidence {
        query: "Shutdown",
        path: b"src/shutdown.rs",
        name: "Shutdown",
    },
    RequiredEvidence {
        query: "shutdown_purge_task",
        path: b"src/db.rs",
        name: "shutdown_purge_task",
    },
    RequiredEvidence {
        query: "into_frame",
        path: b"src/cmd/set.rs",
        name: "into_frame",
    },
    RequiredEvidence {
        query: "set_expires",
        path: b"src/clients/client.rs",
        name: "set_expires",
    },
];

pub struct SearchMetrics {
    pub required_evidence_verified: usize,
    pub warm_p50_us: u64,
    pub warm_p95_us: u64,
}

pub fn verify_manifest_evidence(
    database: &Path,
    repository_identity: &str,
    runs: usize,
    max_warm_query_p95_us: u64,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<SearchMetrics> {
    for required in &REQUIRED_EVIDENCE {
        search_for(database, repository_identity, required, cancelled)?;
    }

    let target = &REQUIRED_EVIDENCE[7];
    let mut warm = Vec::with_capacity(runs);
    for _ in 0..runs {
        let started = Instant::now();
        search_for(database, repository_identity, target, cancelled)?;
        warm.push(
            u64::try_from(started.elapsed().as_micros())
                .map_err(|_| "warm query duration is not representable")?,
        );
    }
    warm.sort_unstable();
    let p50 = nearest_rank(&warm, 50)?;
    let p95 = nearest_rank(&warm, 95)?;
    if p95 > max_warm_query_p95_us {
        return Err("warm query P95 exceeded the proposed budget".into());
    }
    Ok(SearchMetrics {
        required_evidence_verified: REQUIRED_EVIDENCE.len(),
        warm_p50_us: p50,
        warm_p95_us: p95,
    })
}

fn search_for(
    database: &Path,
    repository_identity: &str,
    required: &RequiredEvidence,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<()> {
    let request = LocalCodeSearchRequest::new(database, repository_identity, required.query)
        .with_max_results(SEARCH_RESULTS)?;
    let result = search_local_index(request, Arc::clone(cancelled))?;
    let matched = result.evidence().as_slice().iter().any(|evidence| {
        evidence.identity().path().as_bytes() == required.path
            && matches!(
                evidence.identity().location(),
                EvidenceLocation::SymbolOccurrence(occurrence)
                    if occurrence.name() == required.name
            )
    });
    if !matched {
        return Err(format!(
            "required manifest evidence was not retrieved: {}",
            required.name
        )
        .into());
    }
    if result.coverage().truncated().get() != 0 {
        return Err("required evidence search was silently truncated".into());
    }
    Ok(())
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> ProbeResult<u64> {
    if sorted.is_empty() || percentile == 0 || percentile > 100 {
        return Err("nearest-rank input is invalid".into());
    }
    let numerator = percentile
        .checked_mul(sorted.len())
        .ok_or("percentile rank overflowed")?;
    let rank = numerator
        .checked_add(99)
        .ok_or("percentile rank overflowed")?
        / 100;
    sorted
        .get(rank.saturating_sub(1))
        .copied()
        .ok_or_else(|| "percentile rank is outside the sample".into())
}

#[cfg(test)]
mod tests {
    use super::nearest_rank;

    #[test]
    fn nearest_rank_is_deterministic_and_bounded() {
        assert_eq!(nearest_rank(&[10, 20, 30, 40], 50).expect("p50"), 20);
        assert_eq!(nearest_rank(&[10, 20, 30, 40], 95).expect("p95"), 40);
        assert!(nearest_rank(&[], 50).is_err());
        assert!(nearest_rank(&[1], 0).is_err());
        assert!(nearest_rank(&[1], 101).is_err());
    }
}
