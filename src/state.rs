use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"))
}

fn state_path() -> PathBuf {
    home_dir().join(".fleet-warden/state.json")
}

fn log_path() -> PathBuf {
    home_dir().join(".fleet-warden/log.jsonl")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetSample {
    pub timestamp: String,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupEntry {
    pub date: String,
    pub category: String,
    pub recovered: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    #[serde(default)]
    pub last_cleanups: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub total_recovered: u64,
    #[serde(default)]
    pub budget_samples: Vec<BudgetSample>,
}

impl State {
    pub fn load() -> Result<Self> {
        let path = state_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let state: State = serde_json::from_str(&content)?;
            Ok(state)
        } else {
            let dir = path.parent().unwrap();
            std::fs::create_dir_all(dir)?;
            Ok(State::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = state_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn record_cleanup(&mut self, category: &str, recovered: u64) {
        self.last_cleanups
            .insert(category.to_string(), chrono::Utc::now().to_rfc3339());
        self.total_recovered += recovered;

        // Also append to log
        let entry = CleanupEntry {
            date: chrono::Utc::now().to_rfc3339(),
            category: category.to_string(),
            recovered,
        };

        if let Ok(log_entry) = serde_json::to_string(&entry) {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_path())
            {
                let _ = writeln!(file, "{}", log_entry);
            }
        }
    }
}
