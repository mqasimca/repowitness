#[test]
fn bounded_direct_and_private_ram_first_staging_are_logically_equivalent() -> TestResult {
    let directory = TempDirectory::new()?;
    let facts = fact_fixture(257, 192);

    let direct_path = directory.join("direct.sqlite3");
    let mut direct = open_file_database(&direct_path)?;
    bootstrap_workspace(&direct)?;
    stage_ready_generation(&mut direct, 1, 1, &facts, 17)?;
    activate_generation(&mut direct, 1, 1)?;

    let mut memory = open_memory_database()?;
    bootstrap_workspace(&memory)?;
    stage_ready_generation(&mut memory, 1, 1, &facts, 17)?;
    activate_generation(&mut memory, 1, 1)?;
    let memory_path = directory.join("memory.sqlite3");
    backup_database(&memory, &memory_path)?;
    let materialized_memory = open_read_database(&memory_path)?;

    assert_eq!(
        active_generation_id(&direct)?,
        active_generation_id(&materialized_memory)?
    );
    assert_eq!(
        generation_facts(&direct, 1)?,
        generation_facts(&materialized_memory, 1)?
    );
    Ok(())
}

#[test]
#[ignore = "manual synthetic ingestion timing probe; not a release budget"]
fn benchmark_bounded_direct_against_private_ram_first_staging() -> TestResult {
    let directory = TempDirectory::new()?;
    let facts = fact_fixture(10_000, 256);

    let (direct_elapsed, direct_path) = benchmark_direct(&directory, &facts)?;
    let (memory_elapsed, memory_path) = benchmark_private_ram_first(&directory, &facts)?;

    let direct = open_read_database(&direct_path)?;
    let materialized_memory = open_read_database(&memory_path)?;
    assert_eq!(
        generation_facts(&direct, 1)?,
        generation_facts(&materialized_memory, 1)?
    );
    eprintln!(
        "synthetic SQLite ingestion: direct={direct_elapsed:?} ({} bytes), \
         private-ram-first={memory_elapsed:?} ({} bytes), facts={}",
        fs::metadata(direct_path)?.len(),
        fs::metadata(memory_path)?.len(),
        facts.len()
    );
    Ok(())
}

#[test]
#[ignore = "manual synthetic direct-staging resource probe; not a release budget"]
fn benchmark_bounded_direct_resource_sample() -> TestResult {
    let directory = TempDirectory::new()?;
    let facts = fact_fixture(10_000, 256);
    let (elapsed, database_path) = benchmark_direct(&directory, &facts)?;
    eprintln!(
        "synthetic direct SQLite ingestion: elapsed={elapsed:?}, bytes={}, facts={}, \
         peak_rss_kib={:?}",
        fs::metadata(database_path)?.len(),
        facts.len(),
        peak_resident_set_kib()
    );
    Ok(())
}

#[test]
#[ignore = "manual synthetic RAM-first resource probe; not a release budget"]
fn benchmark_private_ram_first_resource_sample() -> TestResult {
    let directory = TempDirectory::new()?;
    let facts = fact_fixture(10_000, 256);
    let (elapsed, database_path) = benchmark_private_ram_first(&directory, &facts)?;
    eprintln!(
        "synthetic private-RAM-first SQLite ingestion: elapsed={elapsed:?}, bytes={}, facts={}, \
         peak_rss_kib={:?}",
        fs::metadata(database_path)?.len(),
        facts.len(),
        peak_resident_set_kib()
    );
    Ok(())
}

#[test]
#[ignore = "manual synthetic batch/durability timing probe; not a release budget"]
fn benchmark_batch_sizes_and_synchronous_profiles() -> TestResult {
    let directory = TempDirectory::new()?;
    let facts = fact_fixture(10_000, 256);
    for synchronous in ["FULL", "NORMAL"] {
        for batch_limit in [16_usize, 64, 256, 512] {
            let mut elapsed_samples = Vec::with_capacity(5);
            let mut max_wal_bytes = 0_u64;
            for sample in 0..5 {
                let (elapsed, wal_bytes) = benchmark_direct_durability_profile(
                    &directory,
                    &facts,
                    synchronous,
                    batch_limit,
                    sample,
                )?;
                elapsed_samples.push(elapsed);
                max_wal_bytes = max_wal_bytes.max(wal_bytes);
            }
            elapsed_samples.sort_unstable();
            eprintln!(
                "synthetic SQLite durability: synchronous={synchronous}, batch={batch_limit}, \
                 median={:?}, range={:?}..={:?}, max_wal_bytes={max_wal_bytes}",
                elapsed_samples[2], elapsed_samples[0], elapsed_samples[4]
            );
        }
    }
    Ok(())
}
