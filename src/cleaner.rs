use anyhow::Result;
use rayon::prelude::*;
use std::path::PathBuf;

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"))
}

/// Remove all target/ directories under ~/repos
pub fn clean_target_dirs() -> Result<()> {
    let repos = home_dir().join("repos");
    if !repos.is_dir() {
        return Ok(());
    }

    let mut targets = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&repos) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let target = p.join("target");
                if target.is_dir() {
                    targets.push(target);
                }
                // Also check one level deeper
                if let Ok(sub_entries) = std::fs::read_dir(&p) {
                    for sub in sub_entries.flatten() {
                        let sp = sub.path();
                        let target = sp.join("target");
                        if target.is_dir() {
                            targets.push(target);
                        }
                    }
                }
            }
        }
    }

    targets.par_iter().for_each(|t| {
        let _ = std::fs::remove_dir_all(t);
    });

    Ok(())
}

/// Clean pip cache
pub fn clean_pip_cache() -> Result<()> {
    // Try `pip cache purge` first
    let output = std::process::Command::new("pip")
        .args(["cache", "purge"])
        .output();

    match output {
        Ok(o) if o.status.success() => {}
        _ => {
            // Fallback: remove ~/.cache/pip
            let pip_cache = home_dir().join(".cache/pip");
            if pip_cache.is_dir() {
                let _ = std::fs::remove_dir_all(&pip_cache);
            }
        }
    }

    Ok(())
}

/// Clean npm cache
pub fn clean_npm_cache() -> Result<()> {
    let output = std::process::Command::new("npm")
        .args(["cache", "clean", "--force"])
        .output();

    match output {
        Ok(o) if o.status.success() => {}
        _ => {
            // Fallback: remove ~/.npm/_cacache
            let npm_cache = home_dir().join(".npm/_cacache");
            if npm_cache.is_dir() {
                let _ = std::fs::remove_dir_all(&npm_cache);
            }
        }
    }

    Ok(())
}

/// Clean stale session files older than N days
pub fn clean_stale_sessions(days: u64) -> Result<()> {
    let sessions_dir = home_dir().join(".openclaw/agents/main/sessions");
    if !sessions_dir.is_dir() {
        return Ok(());
    }

    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(days * 24 * 60 * 60);

    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Ok(md) = std::fs::symlink_metadata(&p) {
                if md.modified().map_or(false, |t| t < cutoff) {
                    if md.is_dir() {
                        let _ = std::fs::remove_dir_all(&p);
                    } else {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Clean old Rust toolchains (not the active one)
pub fn clean_old_toolchains() -> Result<()> {
    let tc_dir = home_dir().join(".rustup/toolchains");
    if !tc_dir.is_dir() {
        return Ok(());
    }

    // Get active toolchain
    let active = std::process::Command::new("rustup")
        .args(["show", "active-toolchain"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                s.split_whitespace().next().map(|s| s.to_string())
            } else {
                None
            }
        });

    if let Ok(entries) = std::fs::read_dir(&tc_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                let is_active = active.as_ref().map_or(false, |a| name.contains(a));
                if !is_active {
                    let _ = std::fs::remove_dir_all(&p);
                }
            }
        }
    }

    Ok(())
}

/// Clean HuggingFace cache
pub fn clean_huggingface() -> Result<()> {
    let hf_dir = home_dir().join(".cache/huggingface");
    if hf_dir.is_dir() {
        let _ = std::fs::remove_dir_all(&hf_dir);
    }
    Ok(())
}
