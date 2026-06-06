use anyhow::Result;
use std::path::PathBuf;

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"))
}

pub fn run(interval: u64) -> Result<()> {
    println!("🛡️  Fleet Warden watching with {}s interval", interval);
    println!("   Press Ctrl+C to stop");
    println!();

    let log_dir = home_dir().join(".fleet-warden");
    std::fs::create_dir_all(&log_dir)?;

    let state_file = log_dir.join("state.json");
    if !state_file.exists() {
        crate::state::State::default().save()?;
    }

    loop {
        std::thread::sleep(std::time::Duration::from_secs(interval));

        match watch_cycle() {
            Ok(()) => {}
            Err(e) => {
                log::error!("Watch cycle error: {}", e);
            }
        }
    }
}

fn watch_cycle() -> Result<()> {
    let budget = crate::budget::disk_budget()?;

    // Log the check
    let entry = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "event": "watch_check",
        "disk_used_pct": budget.used_pct,
    });
    append_log(&entry)?;

    // Auto-clean target dirs if disk usage > 80%
    if budget.used_pct > 80.0 {
        log::warn!("Disk usage at {:.1}%, auto-cleaning target dirs", budget.used_pct);

        let before = crate::scanner::target_dirs_size()?;
        crate::cleaner::clean_target_dirs()?;
        let after = crate::scanner::target_dirs_size()?;
        let recovered = before.saturating_sub(after);

        let mut state = crate::state::State::load()?;
        state.record_cleanup("target_dirs_auto", recovered);
        state.save()?;

        append_log(&serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "event": "auto_clean",
            "category": "target_dirs",
            "recovered": recovered,
            "disk_used_pct_before": budget.used_pct,
        }))?;

        // If still over 80%, also clean caches
        let new_budget = crate::budget::disk_budget()?;
        if new_budget.used_pct > 80.0 {
            log::warn!("Still at {:.1}%, cleaning caches too", new_budget.used_pct);

            crate::cleaner::clean_pip_cache()?;
            crate::cleaner::clean_npm_cache()?;
        }
    }

    // Save budget sample for growth rate calculation
    save_budget_sample(budget.used, budget.total)?;

    Ok(())
}

fn append_log(entry: &serde_json::Value) -> Result<()> {
    let log_path = home_dir().join(".fleet-warden/log.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    writeln!(file, "{}", entry)?;
    Ok(())
}

fn save_budget_sample(used: u64, total: u64) -> Result<()> {
    let mut state = crate::state::State::load()?;

    // Keep last 30 samples
    state.budget_samples.push(crate::state::BudgetSample {
        timestamp: chrono::Utc::now().to_rfc3339(),
        used_bytes: used,
        total_bytes: total,
    });

    if state.budget_samples.len() > 30 {
        let excess = state.budget_samples.len() - 30;
        state.budget_samples.drain(..excess);
    }

    state.save()?;
    Ok(())
}
