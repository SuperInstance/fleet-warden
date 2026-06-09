use fleet_warden::{budget, cleaner, history, scanner, state, watcher};

use anyhow::Result;
use clap::{Parser, Subcommand};
use console::style;
use humansize::{DECIMAL, format_size};

#[derive(Parser)]
#[command(name = "fleet-warden")]
#[command(about = "Automated disk cleanup daemon for WSL development environments")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan and report what can be cleaned (dry run)
    Check,
    /// Execute cleanup with optional category filters
    Clean {
        /// Clean target/ directories
        #[arg(long)]
        target_dirs: bool,
        /// Clean pip cache
        #[arg(long)]
        pip_cache: bool,
        /// Clean npm cache
        #[arg(long)]
        npm_cache: bool,
        /// Clean stale sessions older than N days
        #[arg(long, value_name = "DAYS")]
        stale_sessions: Option<u64>,
        /// Clean old Rust toolchains
        #[arg(long)]
        old_toolchains: bool,
        /// Clean HuggingFace cache
        #[arg(long)]
        huggingface: bool,
        /// Clean everything
        #[arg(long)]
        all: bool,
    },
    /// Run as daemon, check periodically
    Watch {
        /// Check interval in seconds
        #[arg(long, default_value = "3600")]
        interval: u64,
    },
    /// Show current disk budget
    Budget,
    /// Show cleanup history
    History {
        /// Number of recent entries to show
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Check => cmd_check(),
        Commands::Clean {
            target_dirs,
            pip_cache,
            npm_cache,
            stale_sessions,
            old_toolchains,
            huggingface,
            all,
        } => cmd_clean(CleanTargets {
            target_dirs: target_dirs || all,
            pip_cache: pip_cache || all,
            npm_cache: npm_cache || all,
            stale_sessions: stale_sessions.or(if all { Some(30) } else { None }),
            old_toolchains: old_toolchains || all,
            huggingface: huggingface || all,
            all,
        }),
        Commands::Watch { interval } => cmd_watch(interval),
        Commands::Budget => cmd_budget(),
        Commands::History { limit } => cmd_history(limit),
    }
}

struct CleanTargets {
    target_dirs: bool,
    pip_cache: bool,
    npm_cache: bool,
    stale_sessions: Option<u64>,
    old_toolchains: bool,
    huggingface: bool,
    all: bool,
}

fn cmd_check() -> Result<()> {
    println!("{}", style("🔍 Fleet Warden — Disk Scan Report\n").bold().cyan());

    let report = scanner::full_scan()?;

    println!("{}", style("─".repeat(60)).dim());
    println!("  {:<35} {:>12}  {:>8}", style("Category").bold(), style("Size").bold(), style("Items").bold());
    println!("{}", style("─".repeat(60)).dim());

    let rows = [
        ("Target directories (*/target/)", report.target_dirs_size, report.target_dirs_count),
        ("Pip cache", report.pip_cache_size, report.pip_cache_count),
        ("npm cache", report.npm_cache_size, report.npm_cache_count),
        ("Old Rust toolchains", report.old_toolchains_size, report.old_toolchains_count),
        ("Stale sessions (>30 days)", report.stale_sessions_size, report.stale_sessions_count),
        ("HuggingFace weights", report.huggingface_size, report.huggingface_count),
        ("Large files (>100MB)", report.large_files_size, report.large_files_count),
    ];

    for (label, size, count) in &rows {
        if *size > 0 || *count > 0 {
            println!(
                "  {:<35} {:>12}  {:>8}",
                label,
                format_size(*size, DECIMAL),
                count
            );
        }
    }

    println!("{}", style("─".repeat(60)).dim());
    let total = report.total_cleanable();
    println!(
        "  {:<35} {:>12}",
        style("TOTAL CLEANABLE").bold().yellow(),
        style(format_size(total, DECIMAL)).bold().yellow()
    );
    println!();

    // JSON output to stderr for scripting
    eprintln!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn cmd_clean(targets: CleanTargets) -> Result<()> {
    if !targets.target_dirs && !targets.pip_cache && !targets.npm_cache
        && targets.stale_sessions.is_none() && !targets.old_toolchains
        && !targets.huggingface && !targets.all
    {
        println!("{}", style("No cleanup targets specified. Use --target-dirs, --pip-cache, --npm-cache, --stale-session DAYS, --old-toolchains, --huggingface, or --all").yellow());
        return Ok(());
    }

    println!("{}", style("🧹 Fleet Warden — Cleaning Up\n").bold().green());

    let mut state = state::State::load()?;
    let mut total_recovered: u64 = 0;

    if targets.target_dirs {
        let before = scanner::target_dirs_size()?;
        cleaner::clean_target_dirs()?;
        let after = scanner::target_dirs_size()?;
        let recovered = before.saturating_sub(after);
        total_recovered += recovered;
        println!("  ✓ Target dirs: recovered {}", format_size(recovered, DECIMAL));
        state.record_cleanup("target_dirs", recovered);
    }

    if targets.pip_cache {
        let before = scanner::pip_cache_size()?;
        cleaner::clean_pip_cache()?;
        let after = scanner::pip_cache_size()?;
        let recovered = before.saturating_sub(after);
        total_recovered += recovered;
        println!("  ✓ Pip cache: recovered {}", format_size(recovered, DECIMAL));
        state.record_cleanup("pip_cache", recovered);
    }

    if targets.npm_cache {
        let before = scanner::npm_cache_size()?;
        cleaner::clean_npm_cache()?;
        let after = scanner::npm_cache_size()?;
        let recovered = before.saturating_sub(after);
        total_recovered += recovered;
        println!("  ✓ npm cache: recovered {}", format_size(recovered, DECIMAL));
        state.record_cleanup("npm_cache", recovered);
    }

    if let Some(days) = targets.stale_sessions {
        let before = scanner::stale_sessions_size(days)?;
        cleaner::clean_stale_sessions(days)?;
        let after = scanner::stale_sessions_size(days)?;
        let recovered = before.saturating_sub(after);
        total_recovered += recovered;
        println!("  ✓ Stale sessions (>{days}d): recovered {}", format_size(recovered, DECIMAL));
        state.record_cleanup("stale_sessions", recovered);
    }

    if targets.old_toolchains {
        let before = scanner::old_toolchains_size()?;
        cleaner::clean_old_toolchains()?;
        let after = scanner::old_toolchains_size()?;
        let recovered = before.saturating_sub(after);
        total_recovered += recovered;
        println!("  ✓ Old toolchains: recovered {}", format_size(recovered, DECIMAL));
        state.record_cleanup("old_toolchains", recovered);
    }

    if targets.huggingface {
        let before = scanner::huggingface_size()?;
        cleaner::clean_huggingface()?;
        let after = scanner::huggingface_size()?;
        let recovered = before.saturating_sub(after);
        total_recovered += recovered;
        println!("  ✓ HuggingFace: recovered {}", format_size(recovered, DECIMAL));
        state.record_cleanup("huggingface", recovered);
    }

    state.save()?;
    println!();
    println!(
        "{} Total recovered: {}",
        style("✨").green(),
        style(format_size(total_recovered, DECIMAL)).bold().green()
    );

    Ok(())
}

fn cmd_watch(interval: u64) -> Result<()> {
    watcher::run(interval)
}

fn cmd_budget() -> Result<()> {
    let budget = budget::disk_budget()?;

    println!("{}", style("📊 Fleet Warden — Disk Budget\n").bold().cyan());
    println!("  Mount:      {}", budget.mount_point);
    println!("  Total:      {}", format_size(budget.total, DECIMAL));
    println!("  Used:       {} ({:.1}%)", format_size(budget.used, DECIMAL), budget.used_pct);
    println!("  Free:       {}", format_size(budget.free, DECIMAL));
    println!();

    if let Some(rate) = budget.growth_rate {
        println!("  Growth rate: {}/day (estimated)", format_size(rate, DECIMAL));
        let days_until_full = if rate > 0 { budget.free / rate } else { u64::MAX };
        let days_str = if days_until_full > 365 { "365+".to_string() } else { days_until_full.to_string() };
        println!("  Days until full: {}", days_str);
    } else {
        println!("  Growth rate: unknown (need 2+ data points)");
    }

    println!();
    println!("  Total recovered (all time): {}", format_size(budget.total_recovered, DECIMAL));

    Ok(())
}

fn cmd_history(limit: usize) -> Result<()> {
    let entries = history::load_entries()?;

    if entries.is_empty() {
        println!("{}", style("No cleanup history found.").yellow());
        return Ok(());
    }

    println!("{}", style("📜 Fleet Warden — Cleanup History\n").bold().cyan());
    println!("  {:<22} {:<20} {:>15}", style("Date").bold(), style("Category").bold(), style("Recovered").bold());
    println!("{}", style("─".repeat(60)).dim());

    for entry in entries.iter().rev().take(limit) {
        println!(
            "  {:<22} {:<20} {:>15}",
            entry.date,
            entry.category,
            format_size(entry.recovered, DECIMAL)
        );
    }

    println!("{}", style("─".repeat(60)).dim());
    println!("  Showing {} of {} entries", entries.len().min(limit), entries.len());

    Ok(())
}
