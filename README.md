# fleet-warden

> Automated disk cleanup daemon for WSL development environments.

Prevents disk bloat by scanning and cleaning build artifacts, language caches, old toolchains, stale sessions, and large files.

Based on real cleanup data: **54 GB recovered** from target dirs (10 GB), old toolchains (9 GB), pip cache (16 GB), HuggingFace weights (5 GB), old Node versions, and session trajectories (1 GB).

## Install

```bash
cargo install --path .
```

## Usage

### Check — Dry Run Scan

```bash
# Scan and report what can be cleaned
fleet-warden check
```

Output:

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

JSON report is printed to stderr for scripting:

```bash
fleet-warden check 2>report.json
```

### Clean — Execute Cleanup

```bash
# Clean target dirs only (default)
fleet-warden clean --target-dirs

# Clean specific categories
fleet-warden clean --pip-cache --npm-cache

# Clean stale sessions older than 14 days
fleet-warden clean --stale-sessions 14

# Clean everything
fleet-warden clean --all
```

Each category shows before/after sizes:

```
🧹 Fleet Warden — Cleaning Up

  ✓ Target dirs: recovered 10.2 GB
  ✓ Pip cache: recovered 16.0 GB
  ✓ npm cache: recovered 2.1 GB

✨ Total recovered: 28.3 GB
```

### Watch — Daemon Mode

```bash
# Run with default 1-hour interval
fleet-warden watch

# Custom interval (5 minutes)
fleet-warden watch --interval 300
```

The watcher:
- Checks disk usage every N seconds
- Auto-cleans target dirs when `~/repos` disk usage exceeds 80%
- If still over 80%, also cleans pip and npm caches
- Logs all activity to `~/.fleet-warden/log.jsonl`

### Budget — Disk Analysis

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

### History — Cleanup Log

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
──────────────────────────────────────────────────────────
  Showing 3 of 3 entries
```

## What It Cleans

| Category | Path | Typical Size |
|---|---|---|
| Target directories | `~/repos/*/target/` | 5-15 GB |
| Pip cache | `~/.cache/pip/` | 5-20 GB |
| npm cache | `~/.npm/_cacache/` | 1-5 GB |
| Old Rust toolchains | `~/.rustup/toolchains/` | 5-10 GB |
| Stale sessions | `~/.openclaw/agents/main/sessions/` | 0.5-2 GB |
| HuggingFace weights | `~/.cache/huggingface/` | 2-10 GB |
| Large files | `~/repos/**` (>100MB) | varies |

## State & Logging

- **State file:** `~/.fleet-warden/state.json` — last cleanup dates, total recovered, budget samples
- **Log file:** `~/.fleet-warden/log.jsonl` — append-only audit trail of all cleanups and watch events

## Dependencies

- [clap](https://crates.io/crates/clap) — CLI argument parsing
- [serde](https://crates.io/crates/serde) + [serde_json](https://crates.io/crates/serde_json) — JSON serialization
- [anyhow](https://crates.io/crates/anyhow) — Error handling
- [rayon](https://crates.io/crates/rayon) — Parallel directory scanning
- [chrono](https://crates.io/crates/chrono) — Timestamp handling
- [humansize](https://crates.io/crates/humansize) — Human-readable file sizes
- [console](https://crates.io/crates/console) — Terminal styling
- [indicatif](https://crates.io/crates/indicatif) — Progress bars

## License

MIT
