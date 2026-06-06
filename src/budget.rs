use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct DiskBudget {
    pub mount_point: String,
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub used_pct: f64,
    pub growth_rate: Option<u64>,
    pub total_recovered: u64,
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"))
}

pub fn disk_budget() -> Result<DiskBudget> {
    let path = home_dir();
    let mounts = get_mount_info(&path)?;

    let state = crate::state::State::load().unwrap_or_default();

    // Calculate growth rate from budget samples
    let growth_rate = calculate_growth_rate(&state.budget_samples);

    Ok(DiskBudget {
        mount_point: mounts.mount_point,
        total: mounts.total,
        used: mounts.used,
        free: mounts.free,
        used_pct: mounts.used_pct,
        growth_rate,
        total_recovered: state.total_recovered,
    })
}

struct MountInfo {
    mount_point: String,
    total: u64,
    used: u64,
    free: u64,
    used_pct: f64,
}

fn get_mount_info(path: &std::path::Path) -> Result<MountInfo> {
    // Use statvfs via libc-like approach: read from /proc/mounts and stat
    // For simplicity, use `df` command
    let output = std::process::Command::new("df")
        .args(["-B1", "--output=source,size,used,avail,pcent"])
        .arg(path)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    lines.next(); // skip header

    if let Some(line) = lines.next() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 5 {
            let total: u64 = parts[1].parse().unwrap_or(0);
            let used: u64 = parts[2].parse().unwrap_or(0);
            let free: u64 = parts[3].parse().unwrap_or(0);
            let pct_str = parts[4].trim_end_matches('%');
            let used_pct: f64 = pct_str.parse().unwrap_or(0.0);

            return Ok(MountInfo {
                mount_point: parts[0].to_string(),
                total,
                used,
                free,
                used_pct,
            });
        }
    }

    // Fallback
    Ok(MountInfo {
        mount_point: "/".to_string(),
        total: 0,
        used: 0,
        free: 0,
        used_pct: 0.0,
    })
}

fn calculate_growth_rate(samples: &[crate::state::BudgetSample]) -> Option<u64> {
    if samples.len() < 2 {
        return None;
    }

    let first = &samples[0];
    let last = &samples[samples.len() - 1];

    let t1 = chrono::DateTime::parse_from_rfc3339(&first.timestamp).ok()?;
    let t2 = chrono::DateTime::parse_from_rfc3339(&last.timestamp).ok()?;

    let diff_secs = (t2 - t1).num_seconds();
    if diff_secs <= 0 {
        return None;
    }

    let bytes_growth = if last.used_bytes > first.used_bytes {
        last.used_bytes - first.used_bytes
    } else {
        0
    };

    // Convert to bytes per day
    let days = diff_secs as f64 / 86400.0;
    Some((bytes_growth as f64 / days) as u64)
}
