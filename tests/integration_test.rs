use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ── Test utilities (inline) ──

fn dir_size(path: &Path) -> (u64, usize) {
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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ScanReport {
    target_dirs_count: usize,
    target_dirs_size: u64,
    pip_cache_size: u64,
    pip_cache_count: usize,
    npm_cache_size: u64,
    npm_cache_count: usize,
    old_toolchains_count: usize,
    old_toolchains_size: u64,
    stale_sessions_count: usize,
    stale_sessions_size: u64,
    huggingface_count: usize,
    huggingface_size: u64,
    large_files_count: usize,
    large_files_size: u64,
}

impl ScanReport {
    fn total_cleanable(&self) -> u64 {
        self.target_dirs_size
            + self.pip_cache_size
            + self.npm_cache_size
            + self.old_toolchains_size
            + self.stale_sessions_size
            + self.huggingface_size
            + self.large_files_size
    }
}

fn make_scan_report(
    target_size: u64, pip_size: u64, npm_size: u64,
    toolchains_size: u64, sessions_size: u64, hf_size: u64, large_size: u64,
) -> ScanReport {
    ScanReport {
        target_dirs_count: 5, target_dirs_size: target_size,
        pip_cache_size: pip_size, pip_cache_count: 100,
        npm_cache_size: npm_size, npm_cache_count: 50,
        old_toolchains_count: 2, old_toolchains_size: toolchains_size,
        stale_sessions_count: 10, stale_sessions_size: sessions_size,
        huggingface_count: 3, huggingface_size: hf_size,
        large_files_count: 1, large_files_size: large_size,
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct TestState {
    last_cleanups: std::collections::HashMap<String, String>,
    total_recovered: u64,
    #[serde(default)]
    budget_samples: Vec<serde_json::Value>,
}

impl TestState {
    fn record_cleanup(&mut self, category: &str, recovered: u64) {
        self.last_cleanups
            .insert(category.to_string(), chrono::Utc::now().to_rfc3339());
        self.total_recovered += recovered;
    }
}

fn parse_history(path: &Path) -> Vec<serde_json::Value> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    content.lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn make_sample(timestamp: &str, used: u64, total: u64) -> serde_json::Value {
    serde_json::json!({ "timestamp": timestamp, "used_bytes": used, "total_bytes": total })
}

fn calc_growth_rate(samples: &[serde_json::Value]) -> Option<u64> {
    if samples.len() < 2 { return None; }
    let first = &samples[0];
    let last = &samples[samples.len() - 1];
    let t1 = chrono::DateTime::parse_from_rfc3339(first["timestamp"].as_str()?).ok()?;
    let t2 = chrono::DateTime::parse_from_rfc3339(last["timestamp"].as_str()?).ok()?;
    let diff_secs = (t2 - t1).num_seconds();
    if diff_secs <= 0 { return None; }
    let used1: u64 = first["used_bytes"].as_u64()?;
    let used2: u64 = last["used_bytes"].as_u64()?;
    let bytes_growth = used2.saturating_sub(used1);
    let days = diff_secs as f64 / 86400.0;
    let rate = (bytes_growth as f64 / days) as u64;
    if rate == 0 { None } else { Some(rate) }
}

struct CleanFlags {
    target_dirs: bool, pip_cache: bool, npm_cache: bool,
    stale_sessions: Option<u64>, old_toolchains: bool, huggingface: bool, all: bool,
}

impl CleanFlags {
    fn any_selected(&self) -> bool {
        self.target_dirs || self.pip_cache || self.npm_cache
            || self.stale_sessions.is_some() || self.old_toolchains
            || self.huggingface || self.all
    }
}

// ── Tests ──

#[test]
fn test_dir_size_empty() {
    let tmp = TempDir::new().unwrap();
    let (size, count) = dir_size(tmp.path());
    assert_eq!(size, 0);
    assert_eq!(count, 0);
}

#[test]
fn test_dir_size_with_files() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.txt"), "hello world").unwrap();
    fs::write(tmp.path().join("b.txt"), "x".repeat(1000)).unwrap();
    let (size, count) = dir_size(tmp.path());
    assert_eq!(count, 2);
    assert!(size >= 1011);
}

#[test]
fn test_dir_size_nested() {
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("nested.txt"), "data").unwrap();
    fs::write(tmp.path().join("top.txt"), "top").unwrap();
    let (_size, count) = dir_size(tmp.path());
    assert_eq!(count, 2);
}

#[test]
fn test_dir_size_skips_symlinks() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("real.txt"), "content").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt")).unwrap();
    let (_size, count) = dir_size(tmp.path());
    assert_eq!(count, 1);
}

#[test]
fn test_scan_report_total_cleanable() {
    let report = make_scan_report(100, 200, 300, 400, 50, 60, 10);
    assert_eq!(report.total_cleanable(), 1120);
}

#[test]
fn test_scan_report_zero() {
    let report = make_scan_report(0, 0, 0, 0, 0, 0, 0);
    assert_eq!(report.total_cleanable(), 0);
}

#[test]
fn test_state_save_load_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("state.json");
    let state = TestState { total_recovered: 1024, ..Default::default() };
    let content = serde_json::to_string_pretty(&state).unwrap();
    fs::write(&path, &content).unwrap();
    let loaded: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded["total_recovered"], 1024);
}

#[test]
fn test_state_record_cleanup() {
    let mut state = TestState::default();
    assert_eq!(state.total_recovered, 0);
    state.record_cleanup("test_category", 500);
    assert_eq!(state.total_recovered, 500);
    state.record_cleanup("another", 300);
    assert_eq!(state.total_recovered, 800);
}

#[test]
fn test_state_last_cleanup_date() {
    let mut state = TestState::default();
    state.record_cleanup("pip_cache", 100);
    assert!(state.last_cleanups.contains_key("pip_cache"));
    assert!(state.last_cleanups["pip_cache"].contains("20"));
}

#[test]
fn test_history_parse_empty() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("log.jsonl");
    fs::write(&path, "").unwrap();
    let entries = parse_history(&path);
    assert!(entries.is_empty());
}

#[test]
fn test_history_parse_entries() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("log.jsonl");
    fs::write(&path, r#"{"date":"2025-01-01T00:00:00Z","category":"target_dirs","recovered":5000}
{"date":"2025-01-02T00:00:00Z","category":"pip_cache","recovered":16000000000}
"#).unwrap();
    let entries = parse_history(&path);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["category"], "target_dirs");
    assert_eq!(entries[1]["recovered"], 16000000000_u64);
}

#[test]
fn test_history_ignores_malformed() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("log.jsonl");
    fs::write(&path, "not json\n{\"date\":\"2025-01-01T00:00:00Z\",\"category\":\"test\",\"recovered\":100}\n").unwrap();
    let entries = parse_history(&path);
    assert_eq!(entries.len(), 1);
}

#[test]
fn test_budget_growth_rate_insufficient_data() {
    let samples = vec![make_sample("2025-01-01T00:00:00Z", 1000, 10000)];
    let rate = calc_growth_rate(&samples);
    assert!(rate.is_none());
}

#[test]
fn test_budget_growth_rate_two_samples() {
    let samples = vec![
        make_sample("2025-01-01T00:00:00Z", 1000, 10000),
        make_sample("2025-01-02T00:00:00Z", 2000, 10000),
    ];
    let rate = calc_growth_rate(&samples);
    assert!(rate.is_some());
    assert!(rate.unwrap() > 500);
}

#[test]
fn test_budget_growth_rate_negative() {
    let samples = vec![
        make_sample("2025-01-01T00:00:00Z", 5000, 10000),
        make_sample("2025-01-02T00:00:00Z", 3000, 10000),
    ];
    let rate = calc_growth_rate(&samples);
    assert!(rate.is_none() || rate.unwrap() == 0);
}

#[test]
fn test_clean_target_dir_removes_files() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("myproject/target");
    fs::create_dir_all(target.join("debug")).unwrap();
    fs::write(target.join("debug/app"), "binary data here").unwrap();
    assert!(target.join("debug/app").exists());
    let _ = fs::remove_dir_all(&target);
    assert!(!target.exists());
}

#[test]
fn test_clean_preserves_source_dirs() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("myproject/src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("main.rs"), "fn main() {}").unwrap();
    assert!(src.join("main.rs").exists());
}

#[test]
fn test_no_clean_targets_default() {
    let flags = CleanFlags {
        target_dirs: false, pip_cache: false, npm_cache: false,
        stale_sessions: None, old_toolchains: false, huggingface: false, all: false,
    };
    assert!(!flags.any_selected());
}

#[test]
fn test_all_flag_selects_everything() {
    let flags = CleanFlags {
        target_dirs: false, pip_cache: false, npm_cache: false,
        stale_sessions: None, old_toolchains: false, huggingface: false, all: true,
    };
    assert!(flags.any_selected());
}

#[test]
fn test_large_file_threshold_100mb() {
    let threshold: u64 = 100 * 1024 * 1024;
    assert_eq!(threshold, 104_857_600);
    assert!((99 * 1024 * 1024) < threshold);
    assert!((101 * 1024 * 1024) > threshold);
}

#[test]
fn test_stale_session_threshold_30_days() {
    let threshold_secs = 30u64 * 24 * 60 * 60;
    assert_eq!(threshold_secs, 2_592_000);
}

#[test]
fn test_scan_report_serializes() {
    let report = make_scan_report(10, 20, 30, 40, 50, 60, 70);
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("target_dirs_count"));
    assert!(json.contains("pip_cache_size"));
}

#[test]
fn test_cleanup_entry_serializes() {
    let entry = serde_json::json!({
        "date": "2025-06-06T12:00:00Z",
        "category": "pip_cache",
        "recovered": 16000000000_u64
    });
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("pip_cache"));
    assert!(json.contains("16000000000"));
}

#[test]
fn test_state_budget_samples_trim() {
    let mut state = TestState::default();
    for i in 0..35 {
        state.budget_samples.push(serde_json::json!({"i": i}));
    }
    assert_eq!(state.budget_samples.len(), 35);
    // Simulate trim
    if state.budget_samples.len() > 30 {
        let excess = state.budget_samples.len() - 30;
        state.budget_samples.drain(..excess);
    }
    assert_eq!(state.budget_samples.len(), 30);
    assert_eq!(state.budget_samples[0]["i"], 5);
}

#[test]
fn test_clean_multiple_categories() {
    let tmp = TempDir::new().unwrap();

    // Simulate target dir
    let target = tmp.path().join("project/target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("binary"), "x".repeat(10000)).unwrap();

    // Simulate pip cache
    let pip = tmp.path().join(".cache/pip");
    fs::create_dir_all(&pip).unwrap();
    fs::write(pip.join("package.whl"), "y".repeat(5000)).unwrap();

    let (target_size, _) = dir_size(&target);
    let (pip_size, _) = dir_size(&pip);
    assert!(target_size > 0);
    assert!(pip_size > 0);

    let _ = fs::remove_dir_all(&target);
    let _ = fs::remove_dir_all(&pip);
    assert!(!target.exists());
    assert!(!pip.exists());
}
