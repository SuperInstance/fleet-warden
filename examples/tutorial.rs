//! # Fleet Warden Tutorial
//!
//! A comprehensive guide to using `fleet_warden` as a library for building custom
//! disk cleanup workflows, budget monitors, and automated maintenance pipelines.
//!
//! ## Overview
//!
//! Fleet Warden is a disk cleanup daemon for WSL development environments. It
//! scans for cleanable resources (target directories, caches, stale sessions,
//! old toolchains, HuggingFace weights) and provides both one-shot cleanup and
//! a daemonized watcher mode.
//!
//! This tutorial walks through every public API, from basic scanning to building
//! a fully custom cleanup policy.
//!
//! ## Running the Examples
//!
//! ```sh
//! cargo run --example tutorial
//! ```

use anyhow::Result;
use fleet_warden::{budget, cleaner, history, scanner, state};

// ---------------------------------------------------------------------------
// 1. Scanning — Discover what's consuming disk space
// ---------------------------------------------------------------------------

/// Run a full scan and print the report. This is equivalent to the CLI's
/// `fleet-warden check` command, but gives you the raw [`scanner::ScanReport`]
/// to inspect programmatically.
///
/// The `ScanReport` breaks down disk usage by category:
/// - Target directories (`*/target/`)
/// - Pip cache
/// - npm cache
// - Old Rust toolchains
/// - Stale sessions
/// - HuggingFace model weights
/// - Large files (>100 MB)
fn tutorial_full_scan() -> Result<()> {
    // `full_scan()` walks the filesystem (in parallel where possible) and
    // returns a ScanReport with size + item count per category.
    let report = scanner::full_scan()?;

    // Each category has a `*_count` and `*_size` field.
    println!("Target dirs: {} items, {} bytes", report.target_dirs_count, report.target_dirs_size);
    println!("Pip cache:   {} items, {} bytes", report.pip_cache_count, report.pip_cache_size);
    println!("npm cache:   {} items, {} bytes", report.npm_cache_count, report.npm_cache_size);

    // `total_cleanable()` sums all categories for a single number.
    println!("Total cleanable: {} bytes", report.total_cleanable());

    // The report is `Serialize` — serialize it for logging or APIs.
    let json = serde_json::to_string_pretty(&report)?;
    println!("JSON report:\n{json}");

    Ok(())
}

/// You can also scan individual categories without paying the cost of a full
/// scan. Each category exposes its own `scan_*()` function returning
/// `(count, size)`, plus a convenience `*_size()` function returning just
/// the byte total.
fn tutorial_targeted_scans() -> Result<()> {
    // Just target/ directories
    let (count, size) = scanner::scan_target_dirs()?;
    println!("Target dirs: {count} files, {size} bytes");

    // Just the size, no count
    let pip_size = scanner::pip_cache_size()?;
    println!("Pip cache: {pip_size} bytes");

    // Stale sessions accept a `days` threshold (default is 30)
    let (old_count, old_size) = scanner::scan_stale_sessions(14)?;
    println!("Sessions older than 14 days: {old_count} items, {old_size} bytes");

    // Large files (>100 MB) anywhere under ~/repos
    let (big_count, big_size) = scanner::scan_large_files()?;
    println!("Large files: {big_count} items, {big_size} bytes");

    Ok(())
}

/// The `dir_size()` utility recursively calculates the size and file count of
/// any directory — useful for ad-hoc checks outside the standard categories.
fn tutorial_dir_size() {
    let path = std::path::Path::new("/tmp");
    let (size, count) = scanner::dir_size(path);
    println!("/tmp contains {count} files totaling {size} bytes");
}

// ---------------------------------------------------------------------------
// 2. Cleaning — Reclaim disk space
// ---------------------------------------------------------------------------

/// Each cleaner function targets one category. They are **idempotent** —
/// running them when there's nothing to clean is a no-op.
///
/// A best practice is to measure before and after to compute recovered bytes:
fn tutorial_clean_with_measurement() -> Result<()> {
    // Measure before
    let before = scanner::target_dirs_size()?;

    // Clean all target/ directories under ~/repos (1 level deep)
    cleaner::clean_target_dirs()?;

    // Measure after
    let after = scanner::target_dirs_size()?;
    let recovered = before.saturating_sub(after);
    println!("Recovered {recovered} bytes from target dirs");

    Ok(())
}

/// The cleaner module also exposes dedicated functions for each category:
fn tutorial_all_cleaners() -> Result<()> {
    // Pip cache — tries `pip cache purge`, falls back to removing ~/.cache/pip
    cleaner::clean_pip_cache()?;

    // npm cache — tries `npm cache clean --force`, falls back to ~/.npm/_cacache
    cleaner::clean_npm_cache()?;

    // Stale OpenClaw sessions older than N days
    cleaner::clean_stale_sessions(30)?;

    // Old Rust toolchains — keeps the active one, removes the rest
    cleaner::clean_old_toolchains()?;

    // HuggingFace model cache (~/.cache/huggingface)
    cleaner::clean_huggingface()?;

    println!("All cleaners ran successfully.");
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. State — Persistent bookkeeping
// ---------------------------------------------------------------------------

/// Fleet Warden persists state to `~/.fleet-warden/state.json`. The [`state::State`]
/// struct tracks cleanup history and budget samples.
///
/// Key types:
/// - [`state::State`] — top-level state: last cleanups, total recovered, budget samples
/// - [`state::BudgetSample`] — a single disk-usage measurement (timestamp + bytes)
/// - [`state::CleanupEntry`] — a cleanup event (date + category + bytes recovered)
fn tutorial_state_management() -> Result<()> {
    // Load existing state (or create default if none exists)
    let mut s = state::State::load()?;

    println!("Total bytes recovered (all time): {}", s.total_recovered);
    println!("Budget samples on file: {}", s.budget_samples.len());

    // Record a cleanup event — this updates both in-memory state and appends
    // a JSONL log entry to ~/.fleet-warden/log.jsonl
    s.record_cleanup("pip_cache", 1_048_576);
    println!("Recorded a 1 MB pip cache cleanup");

    // Persist state back to disk
    s.save()?;

    // Budget samples are just (timestamp, used_bytes, total_bytes) tuples
    for sample in &s.budget_samples {
        println!(
            "  {} — used: {}, total: {}",
            sample.timestamp, sample.used_bytes, sample.total_bytes
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 4. Budget — Disk usage and growth rate
// ---------------------------------------------------------------------------

/// The [`budget::DiskBudget`] struct gives you a snapshot of disk usage plus
/// an estimated growth rate (bytes/day), derived from the budget samples in
/// state.
fn tutorial_disk_budget() -> Result<()> {
    let b = budget::disk_budget()?;

    println!("Mount point:  {}", b.mount_point);
    println!("Total:        {} bytes", b.total);
    println!("Used:         {} bytes ({:.1}%)", b.used, b.used_pct);
    println!("Free:         {} bytes", b.free);
    println!("Recovered:    {} bytes (all time)", b.total_recovered);

    // Growth rate requires at least 2 budget samples in state.
    // The watcher daemon collects these automatically.
    match b.growth_rate {
        Some(rate) => {
            println!("Growth rate:  {rate} bytes/day");
            if rate > 0 {
                let days_left = b.free / rate;
                println!("Days until full: {days_left}");
            }
        }
        None => println!("Growth rate: unknown (need 2+ data points)"),
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 5. History — Past cleanup events
// ---------------------------------------------------------------------------

/// Cleanup events are logged as JSONL in `~/.fleet-warden/log.jsonl`.
/// The [`history`] module reads them back as [`history::HistoryEntry`] structs.
fn tutorial_cleanup_history() -> Result<()> {
    let entries = history::load_entries()?;

    println!("Found {} cleanup entries", entries.len());

    for entry in entries.iter().rev().take(5) {
        println!(
            "  {} — {} recovered {} bytes",
            entry.date, entry.category, entry.recovered
        );
    }

    // Aggregate: total recovered by category
    let mut by_category = std::collections::HashMap::new();
    for entry in &entries {
        *by_category.entry(entry.category.clone()).or_insert(0u64) += entry.recovered;
    }
    println!("\nRecovered by category:");
    for (cat, total) in &by_category {
        println!("  {cat}: {total} bytes");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 6. Building a Custom Policy — Combine scan + clean + state
// ---------------------------------------------------------------------------

/// A custom cleanup policy that only cleans when disk usage exceeds a
/// threshold, and only targets the categories that would recover the most
/// space.
fn tutorial_custom_policy(threshold_pct: f64) -> Result<()> {
    println!("=== Custom Cleanup Policy (threshold: {threshold_pct}%) ===\n");

    // Step 1: Check current budget
    let b = budget::disk_budget()?;
    println!("Disk usage: {:.1}%", b.used_pct);

    if b.used_pct < threshold_pct {
        println!("Below threshold — nothing to do.");
        return Ok(());
    }

    // Step 2: Scan to find the biggest offenders
    let report = scanner::full_scan()?;

    let mut state = state::State::load()?;
    let mut total_recovered: u64 = 0;

    // Step 3: Clean categories that exceed a size threshold (e.g., >500 MB)
    let min_clean_size = 500 * 1024 * 1024;

    if report.target_dirs_size > min_clean_size {
        let before = scanner::target_dirs_size()?;
        cleaner::clean_target_dirs()?;
        let recovered = before.saturating_sub(scanner::target_dirs_size()?);
        total_recovered += recovered;
        state.record_cleanup("target_dirs", recovered);
        println!("Cleaned target dirs: recovered {recovered} bytes");
    }

    if report.pip_cache_size > min_clean_size {
        let before = scanner::pip_cache_size()?;
        cleaner::clean_pip_cache()?;
        let recovered = before.saturating_sub(scanner::pip_cache_size()?);
        total_recovered += recovered;
        state.record_cleanup("pip_cache", recovered);
        println!("Cleaned pip cache: recovered {recovered} bytes");
    }

    if report.npm_cache_size > min_clean_size {
        let before = scanner::npm_cache_size()?;
        cleaner::clean_npm_cache()?;
        let recovered = before.saturating_sub(scanner::npm_cache_size()?);
        total_recovered += recovered;
        state.record_cleanup("npm_cache", recovered);
        println!("Cleaned npm cache: recovered {recovered} bytes");
    }

    // Step 4: Persist state and report
    state.save()?;
    println!("\nTotal recovered: {total_recovered} bytes");

    // Step 5: Show updated budget
    let new_budget = budget::disk_budget()?;
    println!("New disk usage: {:.1}%", new_budget.used_pct);

    Ok(())
}

// ---------------------------------------------------------------------------
// Main — Run all tutorials
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    println!("╔══════════════════════════════════════════════════╗");
    println!("║         Fleet Warden — Library Tutorial          ║");
    println!("╚══════════════════════════════════════════════════╝\n");

    println!("── 1. Full Scan ──");
    tutorial_full_scan()?;
    println!();

    println!("── 2. Targeted Scans ──");
    tutorial_targeted_scans()?;
    println!();

    println!("── 3. Dir Size Utility ──");
    tutorial_dir_size();
    println!();

    println!("── 4. Clean with Measurement ──");
    // tutorial_clean_with_measurement()?; // Uncomment to actually clean
    println!("(skipped — uncomment to run)\n");

    println!("── 5. All Cleaners ──");
    // tutorial_all_cleaners()?; // Uncomment to actually clean
    println!("(skipped — uncomment to run)\n");

    println!("── 6. State Management ──");
    tutorial_state_management()?;
    println!();

    println!("── 7. Disk Budget ──");
    tutorial_disk_budget()?;
    println!();

    println!("── 8. Cleanup History ──");
    tutorial_cleanup_history()?;
    println!();

    println!("── 9. Custom Policy (85% threshold) ──");
    // tutorial_custom_policy(85.0)?; // Uncomment to run with real cleanup
    println!("(skipped — uncomment to run)\n");

    println!("✅ Tutorial complete!");
    Ok(())
}
