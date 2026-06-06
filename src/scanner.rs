use anyhow::Result;
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Serialize, Clone)]
pub struct ScanReport {
    pub target_dirs_count: usize,
    pub target_dirs_size: u64,
    pub pip_cache_size: u64,
    pub pip_cache_count: usize,
    pub npm_cache_size: u64,
    pub npm_cache_count: usize,
    pub old_toolchains_count: usize,
    pub old_toolchains_size: u64,
    pub stale_sessions_count: usize,
    pub stale_sessions_size: u64,
    pub huggingface_count: usize,
    pub huggingface_size: u64,
    pub large_files_count: usize,
    pub large_files_size: u64,
}

impl ScanReport {
    pub fn total_cleanable(&self) -> u64 {
        self.target_dirs_size
            + self.pip_cache_size
            + self.npm_cache_size
            + self.old_toolchains_size
            + self.stale_sessions_size
            + self.huggingface_size
            + self.large_files_size
    }
}

pub fn full_scan() -> Result<ScanReport> {
    let (td_count, td_size) = scan_target_dirs()?;
    let (pip_count, pip_size) = scan_pip_cache()?;
    let (npm_count, npm_size) = scan_npm_cache()?;
    let (tc_count, tc_size) = scan_old_toolchains()?;
    let (ss_count, ss_size) = scan_stale_sessions(30)?;
    let (hf_count, hf_size) = scan_huggingface()?;
    let (lf_count, lf_size) = scan_large_files()?;

    Ok(ScanReport {
        target_dirs_count: td_count,
        target_dirs_size: td_size,
        pip_cache_size: pip_size,
        pip_cache_count: pip_count,
        npm_cache_size: npm_size,
        npm_cache_count: npm_count,
        old_toolchains_count: tc_count,
        old_toolchains_size: tc_size,
        stale_sessions_count: ss_count,
        stale_sessions_size: ss_size,
        huggingface_count: hf_count,
        huggingface_size: hf_size,
        large_files_count: lf_count,
        large_files_size: lf_size,
    })
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"))
}

/// Recursively calculate directory size and file count
pub fn dir_size(path: &Path) -> (u64, usize) {
    let mut total_size: u64 = 0;
    let mut count: usize = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Ok(md) = std::fs::symlink_metadata(&p) {
                if md.is_file() {
                    total_size += md.len();
                    count += 1;
                } else if md.is_dir() && !md.is_symlink() {
                    let (s, c) = dir_size(&p);
                    total_size += s;
                    count += c;
                }
            }
        }
    }
    (total_size, count)
}

/// Find all target/ directories under ~/repos
fn find_target_dirs() -> Vec<PathBuf> {
    let repos = home_dir().join("repos");
    if !repos.is_dir() {
        return vec![];
    }

    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&repos) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let target = p.join("target");
                if target.is_dir() {
                    results.push(target);
                }
                // Also check one level deeper for workspaces
                if let Ok(sub_entries) = std::fs::read_dir(&p) {
                    for sub in sub_entries.flatten() {
                        let sp = sub.path();
                        let target = sp.join("target");
                        if target.is_dir() {
                            results.push(target);
                        }
                    }
                }
            }
        }
    }
    results
}

pub fn scan_target_dirs() -> Result<(usize, u64)> {
    let targets = find_target_dirs();
    let results: Vec<(u64, usize)> = targets
        .par_iter()
        .map(|p| {
            let (s, c) = dir_size(p);
            (s, c)
        })
        .collect();

    let total_size: u64 = results.iter().map(|(s, _)| *s).sum();
    let total_count: usize = results.iter().map(|(_, c)| *c).sum();
    Ok((total_count, total_size))
}

pub fn target_dirs_size() -> Result<u64> {
    let (_, size) = scan_target_dirs()?;
    Ok(size)
}

fn get_pip_cache_dir() -> PathBuf {
    // Try `pip cache dir` first, fall back to ~/.cache/pip
    if let Ok(output) = std::process::Command::new("pip")
        .args(["cache", "dir"])
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let p = PathBuf::from(&s);
            if p.is_dir() {
                return p;
            }
        }
    }
    home_dir().join(".cache/pip")
}

pub fn scan_pip_cache() -> Result<(usize, u64)> {
    let pip_dir = get_pip_cache_dir();
    if pip_dir.is_dir() {
        let (size, count) = dir_size(&pip_dir);
        Ok((count, size))
    } else {
        Ok((0, 0))
    }
}

pub fn pip_cache_size() -> Result<u64> {
    let (_, size) = scan_pip_cache()?;
    Ok(size)
}

pub fn scan_npm_cache() -> Result<(usize, u64)> {
    let npm_dir = home_dir().join(".npm/_cacache");
    if npm_dir.is_dir() {
        let (size, count) = dir_size(&npm_dir);
        Ok((count, size))
    } else {
        Ok((0, 0))
    }
}

pub fn npm_cache_size() -> Result<u64> {
    let (_, size) = scan_npm_cache()?;
    Ok(size)
}

/// Get the active Rust toolchain name
fn get_active_toolchain() -> Option<String> {
    let output = std::process::Command::new("rustup")
        .args(["show", "active-toolchain"])
        .output()
        .ok()?;

    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Output is like "stable-x86_64-unknown-linux-gnu (default)"
        Some(s.split_whitespace().next()?.to_string())
    } else {
        None
    }
}

fn get_toolchain_dir() -> PathBuf {
    home_dir().join(".rustup/toolchains")
}

pub fn scan_old_toolchains() -> Result<(usize, u64)> {
    let tc_dir = get_toolchain_dir();
    if !tc_dir.is_dir() {
        return Ok((0, 0));
    }

    let active = get_active_toolchain();
    let mut total_size: u64 = 0;
    let mut count: usize = 0;

    if let Ok(entries) = std::fs::read_dir(&tc_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                let is_active = active.as_ref().map_or(false, |a| name.contains(a));
                if !is_active {
                    let (s, c) = dir_size(&p);
                    total_size += s;
                    count += c;
                }
            }
        }
    }

    Ok((count, total_size))
}

pub fn old_toolchains_size() -> Result<u64> {
    let (_, size) = scan_old_toolchains()?;
    Ok(size)
}

fn get_sessions_dir() -> PathBuf {
    home_dir().join(".openclaw/agents/main/sessions")
}

pub fn scan_stale_sessions(days: u64) -> Result<(usize, u64)> {
    let sessions_dir = get_sessions_dir();
    if !sessions_dir.is_dir() {
        return Ok((0, 0));
    }

    let cutoff = SystemTime::now()
        - std::time::Duration::from_secs(days * 24 * 60 * 60);

    let mut total_size: u64 = 0;
    let mut count: usize = 0;

    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Ok(md) = std::fs::symlink_metadata(&p) {
                if md.modified().map_or(false, |t| t < cutoff) {
                    if md.is_file() {
                        total_size += md.len();
                        count += 1;
                    } else if md.is_dir() {
                        let (s, c) = dir_size(&p);
                        total_size += s;
                        count += c;
                    }
                }
            }
        }
    }

    Ok((count, total_size))
}

pub fn stale_sessions_size(days: u64) -> Result<u64> {
    let (_, size) = scan_stale_sessions(days)?;
    Ok(size)
}

pub fn scan_huggingface() -> Result<(usize, u64)> {
    let hf_dir = home_dir().join(".cache/huggingface");
    if hf_dir.is_dir() {
        let (size, count) = dir_size(&hf_dir);
        Ok((count, size))
    } else {
        Ok((0, 0))
    }
}

pub fn huggingface_size() -> Result<u64> {
    let (_, size) = scan_huggingface()?;
    Ok(size)
}

/// Find large files (>100MB) in ~/repos
pub fn scan_large_files() -> Result<(usize, u64)> {
    let repos = home_dir().join("repos");
    if !repos.is_dir() {
        return Ok((0, 0));
    }

    const THRESHOLD: u64 = 100 * 1024 * 1024; // 100 MB
    let mut count: usize = 0;
    let mut total_size: u64 = 0;

    scan_large_files_recursive(&repos, THRESHOLD, &mut count, &mut total_size);

    Ok((count, total_size))
}

fn scan_large_files_recursive(dir: &Path, threshold: u64, count: &mut usize, total_size: &mut u64) {
    // Skip known cache/build dirs
    let skip = |name: &str| -> bool {
        matches!(name, "target" | "node_modules" | ".git" | "__pycache__" | ".venv" | "venv")
    };

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            if skip(&name) {
                continue;
            }
            if let Ok(md) = std::fs::symlink_metadata(&p) {
                if md.is_file() && md.len() > threshold {
                    *count += 1;
                    *total_size += md.len();
                } else if md.is_dir() && !md.is_symlink() {
                    scan_large_files_recursive(&p, threshold, count, total_size);
                }
            }
        }
    }
}
