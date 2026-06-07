# fleet-warden

54 GB recovered. That's what fleet-warden found on a real WSL development environment: 10 GB in Rust `target/` dirs, 16 GB in pip cache, 9 GB in old toolchains, 5 GB in HuggingFace weights, and more.

Disk fills up silently. Build artifacts pile up. Language caches grow unchecked. Old toolchains stick around. You don't notice until `df -h` shows 95% and everything grinds to a halt.

fleet-warden watches. It scans, reports, cleans, and keeps watching.

## Install

```bash
cargo install --path .
```

## Check: See What's Wasting Space

```bash
fleet-warden check
```

That one command scans your entire development environment and prints a report:

```
🔍 Fleet Warden — Disk Scan Report

──────────────────────────────────────────────────────────
  Category                               Size      Items
──────────────────────────────────────────────────────────
  Target directories (*/target/)        10.2 GB       47
  Pip cache                             16.0 GB    1243
  npm cache                              2.1 GB      89
  Old Rust toolchains                    9.4 GB        3
  Stale sessions (>30 days)              1.1 GB      56
  HuggingFace weights                    5.3 GB       12
  Large files (>100MB)                   4.7 GB        3
──────────────────────────────────────────────────────────
  TOTAL CLEANABLE                       48.8 GB
```

A JSON report also goes to stderr for scripting:

```bash
fleet-warden check 2>report.json
```

**The code behind it** — here's what the scanner actually does:

```rust
// The scanner parallel-scans directories using rayon
use rayon::prelude::*;

fn scan_target_dirs() -> Result<(usize, u64)> {
    let targets = find_target_dirs(); // ~/repos/*/target/
    let results: Vec<(u64, usize)> = targets
        .par_iter()                    // parallel scan
        .map(|p| dir_size(p))          // recursive directory size
        .collect();

    let total_size: u64 = results.iter().map(|(s, _)| *s).sum();
    let total_count: usize = results.iter().map(|(_, c)| *c).sum();
    Ok((total_count, total_size))
}
```

Every category uses the same pattern: find the paths, scan in parallel, sum the sizes.

## Clean: Take Out the Trash

```bash
# Clean specific categories
fleet-warden clean --target-dirs
fleet-warden clean --pip-cache --npm-cache
fleet-warden clean --stale-sessions 14
fleet-warden clean --old-toolchains
fleet-warden clean --huggingface

# Nuclear option: clean everything
fleet-warden clean --all
```

Each clean measures before and after, so you see exactly what was recovered:

```
🧹 Fleet Warden — Cleaning Up

  ✓ Target dirs: recovered 10.2 GB
  ✓ Pip cache: recovered 16.0 GB
  ✓ npm cache: recovered 2.1 GB

✨ Total recovered: 28.3 GB
```

**How cleaning works** — it's not magic. Each category has a targeted strategy:

```rust
// Target dirs: parallel delete
pub fn clean_target_dirs() -> Result<()> {
    let targets = find_target_dirs();
    targets.par_iter().for_each(|t| {
        let _ = std::fs::remove_dir_all(t);
    });
    Ok(())
}

// Pip cache: use pip's own purge command, fallback to rm
pub fn clean_pip_cache() -> Result<()> {
    let output = std::process::Command::new("pip")
        .args(["cache", "purge"])
        .output();
    match output {
        Ok(o) if o.status.success() => {} // pip cleaned it
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

// Old toolchains: keep the active one, remove the rest
pub fn clean_old_toolchains() -> Result<()> {
    let active = get_active_toolchain(); // "stable-x86_64-unknown-linux-gnu"
    // Remove every toolchain that isn't the active one
    for entry in fs::read_dir(tc_dir)? {
        let name = entry.file_name();
        if !name.contains(&active) {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}
```

## Watch: The Daemon That Never Sleeps

```bash
# Run with default 1-hour interval
fleet-warden watch

# Custom interval (every 5 minutes)
fleet-warden watch --interval 300
```

The watcher runs a loop:

1. Check disk usage
2. If usage exceeds 80%, clean target dirs
3. If still over 80%, also clean pip and npm caches
4. Log everything to `~/.fleet-warden/log.jsonl`

**The watch cycle in code:**

```rust
fn watch_cycle() -> Result<()> {
    let budget = disk_budget()?;

    // Log the check
    append_log(&json!({
        "event": "watch_check",
        "disk_used_pct": budget.used_pct,
    }))?;

    // Auto-clean target dirs if disk usage > 80%
    if budget.used_pct > 80.0 {
        let before = target_dirs_size()?;
        clean_target_dirs()?;
        let after = target_dirs_size()?;
        let recovered = before.saturating_sub(after);

        append_log(&json!({
            "event": "auto_clean",
            "category": "target_dirs",
            "recovered": recovered,
        }))?;

        // If still over 80%, escalate
        let new_budget = disk_budget()?;
        if new_budget.used_pct > 80.0 {
            clean_pip_cache()?;
            clean_npm_cache()?;
        }
    }

    save_budget_sample(budget.used, budget.total)?;
    Ok(())
}
```

Escalation strategy: target dirs first (safest, just build artifacts), then caches (re-downloable). Never touches source code.

## Budget: Know Your Disk Trajectory

```bash
fleet-warden budget
```

```
📊 Fleet Warden — Disk Budget

  Mount:      /dev/sdb
  Total:      250.0 GB
  Used:       187.5 GB (75.0%)
  Free:       62.5 GB

  Growth rate: 500 MB/day (estimated)
  Days until full: 125

  Total recovered (all time): 54.2 GB
```

The growth rate is calculated from budget samples over time. It takes two data points to estimate:

```rust
fn calculate_growth_rate(samples: &[BudgetSample]) -> Option<u64> {
    if samples.len() < 2 {
        return None;  // need at least 2 samples
    }

    let first = &samples[0];
    let last = &samples[samples.len() - 1];

    let diff_secs = (last.timestamp - first.timestamp).num_seconds();
    let bytes_growth = last.used_bytes.saturating_sub(first.used_bytes);

    // Convert to bytes per day
    let days = diff_secs as f64 / 86400.0;
    Some((bytes_growth as f64 / days) as u64)
}
```

The more you run `watch`, the more accurate the growth rate becomes. After a few days, you'll see "Days until full: 47" and know it's time to act.

## History: The Cleanup Audit Trail

```bash
fleet-warden history
fleet-warden history --limit 10
```

```
📜 Fleet Warden — Cleanup History

  Date                   Category              Recovered
──────────────────────────────────────────────────────────
  2025-06-06T13:00:00Z   target_dirs               10.2 GB
  2025-06-05T09:30:00Z   pip_cache                  16.0 GB
  2025-06-04T14:00:00Z   huggingface                 5.3 GB
  2025-06-03T20:00:00Z   target_dirs_auto            3.7 GB
──────────────────────────────────────────────────────────
  Showing 4 of 4 entries
```

Every cleanup — manual or automatic — is logged. The history is append-only JSONL:

```json
{"date":"2025-06-06T13:00:00Z","category":"target_dirs","recovered":10945257472}
{"date":"2025-06-05T09:30:00Z","category":"pip_cache","recovered":17179869184}
```

This is your audit trail. You can always see what was cleaned and when.

## What Gets Cleaned

| Category | Path | Strategy | Typical Size |
|---|---|---|---|
| Target directories | `~/repos/*/target/` | Full recursive delete | 5-15 GB |
| Pip cache | `~/.cache/pip/` | `pip cache purge` or rm | 5-20 GB |
| npm cache | `~/.npm/_cacache/` | `npm cache clean --force` or rm | 1-5 GB |
| Old Rust toolchains | `~/.rustup/toolchains/` | Keep active, remove rest | 5-10 GB |
| Stale sessions | `~/.openclaw/agents/main/sessions/` | Files older than N days | 0.5-2 GB |
| HuggingFace weights | `~/.cache/huggingface/` | Full delete | 2-10 GB |
| Large files | `~/repos/**` (>100MB) | Report only (no auto-delete) | varies |

**Safety guarantees:**
- Never touches source files
- Never touches `.git` directories
- Toolchain cleaning preserves the active toolchain
- Session cleaning only removes files older than the threshold
- Large files are reported but never auto-deleted

## The Scanner: What It Checks

```rust
pub struct ScanReport {
    pub target_dirs_count: usize,     // number of target/ dirs found
    pub target_dirs_size: u64,        // total bytes
    pub pip_cache_size: u64,          // ~/.cache/pip
    pub npm_cache_size: u64,          // ~/.npm/_cacache
    pub old_toolchains_count: usize,  // non-active Rust toolchains
    pub old_toolchains_size: u64,     // their total size
    pub stale_sessions_count: usize,  // old session files
    pub stale_sessions_size: u64,
    pub huggingface_count: usize,     // HF model weights
    pub huggingface_size: u64,
    pub large_files_count: usize,     // files > 100MB
    pub large_files_size: u64,
}
```

Each category is scanned independently. The `full_scan()` function runs them all and aggregates:

```rust
pub fn full_scan() -> Result<ScanReport> {
    let (td_count, td_size) = scan_target_dirs()?;
    let (pip_count, pip_size) = scan_pip_cache()?;
    let (npm_count, npm_size) = scan_npm_cache()?;
    let (tc_count, tc_size) = scan_old_toolchains()?;
    let (ss_count, ss_size) = scan_stale_sessions(30)?;
    let (hf_count, hf_size) = scan_huggingface()?;
    let (lf_count, lf_size) = scan_large_files()?;
    // ... aggregate into ScanReport
}
```

## State Management

fleet-warden persists state in `~/.fleet-warden/`:

```
~/.fleet-warden/
├── state.json      # Last cleanup dates, total recovered, budget samples
└── log.jsonl       # Append-only audit trail
```

**State file** tracks:
- When each category was last cleaned
- Total bytes recovered across all time
- Budget samples for growth rate estimation

```rust
#[derive(Default, Serialize, Deserialize)]
pub struct State {
    pub last_cleanups: HashMap<String, String>,  // category → ISO timestamp
    pub total_recovered: u64,                     // cumulative bytes
    pub budget_samples: Vec<BudgetSample>,        // last 30 samples
}
```

## Architecture

```
fleet-warden check    → scanner.rs → ScanReport
fleet-warden clean    → scanner.rs (before) → cleaner.rs → scanner.rs (after)
fleet-warden watch    → watcher.rs → loop { budget → clean → log }
fleet-warden budget   → budget.rs → DiskBudget
fleet-warden history  → history.rs → Vec<HistoryEntry>
                      ↕ state.rs (persistent state)
```

Each module is independent. `scanner` only reads. `cleaner` only writes (deletes). `state` tracks. `watcher` orchestrates.

## Dependencies

| Crate | Purpose |
|-------|---------|
| [clap](https://crates.io/crates/clap) | CLI argument parsing |
| [serde](https://crates.io/crates/serde) + [serde_json](https://crates.io/crates/serde_json) | JSON state/logs |
| [rayon](https://crates.io/crates/rayon) | Parallel directory scanning |
| [chrono](https://crates.io/crates/chrono) | Timestamps |
| [humansize](https://crates.io/crates/humansize) | Human-readable sizes |
| [console](https://crates.io/crates/console) | Terminal colors |
| [indicatif](https://crates.io/crates/indicatif) | Progress bars |
| [dirs](https://crates.io/crates/dirs) | Cross-platform home directory |

## License

MIT
