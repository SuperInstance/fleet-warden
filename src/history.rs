use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"))
}

#[derive(Debug, Clone, Deserialize)]
pub struct HistoryEntry {
    pub date: String,
    pub category: String,
    pub recovered: u64,
}

pub fn load_entries() -> Result<Vec<HistoryEntry>> {
    let log_path = home_dir().join(".fleet-warden/log.jsonl");
    if !log_path.exists() {
        return Ok(vec![]);
    }

    let content = std::fs::read_to_string(&log_path)?;
    let mut entries = Vec::new();

    for line in content.lines() {
        if let Ok(entry) = serde_json::from_str::<HistoryEntry>(line) {
            entries.push(entry);
        }
    }

    Ok(entries)
}
