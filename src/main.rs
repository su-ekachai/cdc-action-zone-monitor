//! # CDC Action Zone Daily Alert
//!
//! A lightweight CLI scanner that monitors a configurable watchlist of assets for
//! CDC Action Zone buy/sell signals (EMA 12/26 crossover) and delivers Telegram alerts.
//!
//! The binary is designed for resource-constrained environments
//! (Raspberry Pi, $5/mo VPS with 1 CPU and 512 MB RAM).
//!
//! ## Architecture
//!
//! Single binary, no daemon. Cron triggers the scan → fetch daily OHLCV data →
//! detect EMA crossovers → send alerts for new signals → exit.
//!
//! ## Modules
//!
//! - [`config`] — TOML file settings + environment variable secrets (12-Factor)
//! - [`watchlist`] — `[[watchlist]]` array management in `config.toml`
//! - [`state`] — JSON-persisted last-signal tracking to prevent duplicate alerts
//! - [`data`] — Market data providers (Yahoo Finance, Binance)
//! - [`signals`] — Technical indicators and CDC Action Zone crossover detection
//! - [`alerts`] — Telegram alert delivery

mod alerts;
mod config;
mod data;
mod signals;
mod state;
mod watchlist;

use std::io::{IsTerminal, Write};
use std::time::Instant;

use clap::{ArgAction, CommandFactory, Parser, Subcommand, ValueHint};
use serde::Serialize;

use crate::config::Config;
use crate::data::DataProvider;
use crate::data::binance::BinanceProvider;
use crate::data::yahoo::YahooProvider;
use crate::signals::cdc_zone;
use crate::state::StateStore;
use crate::watchlist::DataSource;

/// Color output mode for terminal formatting.
#[derive(Clone, Copy, clap::ValueEnum)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

/// CDC Action Zone Monitor — scans assets for EMA crossover signals.
#[derive(Parser)]
#[command(name = "cdc-az-daily-alert")]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
struct Args {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml", value_hint = ValueHint::FilePath, global = true)]
    config: String,

    /// Increase verbosity (-v = debug, -vv = trace)
    #[arg(short, long, action = ArgAction::Count, global = true)]
    verbose: u8,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Color output mode
    #[arg(long, default_value = "auto", global = true)]
    color: ColorMode,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Scan watchlist for signals and send alerts (default if no subcommand)
    Scan {
        /// Output results as JSON to stdout
        #[arg(long)]
        json: bool,

        /// Analyze signals but don't send Telegram alerts
        #[arg(long)]
        dry_run: bool,
    },
    /// Add a symbol to the watchlist
    Add {
        /// Symbol to add (e.g., TSLA, SOL-USD)
        symbol: String,

        /// Data source: yahoo or binance (auto-detected if omitted)
        #[arg(short, long)]
        source: Option<String>,
    },
    /// Remove a symbol from the watchlist
    Remove {
        /// Symbol to remove
        symbol: String,
    },
    /// List all symbols in the watchlist
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Validate configuration and connectivity
    Check {
        /// Also verify Telegram bot token works
        #[arg(long)]
        telegram: bool,
    },
    /// Show last scan results from state file
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate for (bash, zsh, fish, powershell, elvish)
        shell: clap_complete::Shell,
    },
}

#[derive(Serialize)]
struct ScanOutput {
    timestamp: String,
    symbols_scanned: usize,
    signals: Vec<SignalOutput>,
    errors: Vec<String>,
    duration_ms: u64,
}

#[derive(Serialize)]
struct SignalOutput {
    symbol: String,
    signal: String,
    zone: String,
    price: f64,
    rsi: f64,
    volume_ratio: f64,
    alerted: bool,
}

fn main() {
    let args = Args::parse();

    match args.color {
        ColorMode::Always => owo_colors::set_override(true),
        ColorMode::Never => owo_colors::set_override(false),
        ColorMode::Auto => {}
    }

    let filter = if args.quiet {
        log::LevelFilter::Error
    } else {
        match args.verbose {
            0 => log::LevelFilter::Info,
            1 => log::LevelFilter::Debug,
            _ => log::LevelFilter::Trace,
        }
    };

    let mut builder = if std::env::var("RUST_LOG").is_ok() {
        env_logger::Builder::from_env(env_logger::Env::default())
    } else {
        let mut b = env_logger::Builder::new();
        b.filter_level(filter);
        b
    };

    builder
        .format(|buf, record| {
            let ts = buf.timestamp_millis();
            let level = record.level();
            let module = record.module_path().unwrap_or("-");
            let short_module = module
                .strip_prefix("cdc_az_daily_alert::")
                .unwrap_or(module);
            writeln!(
                buf,
                "{ts} [{level:<5}] {short_module:<20} | {}",
                record.args()
            )
        })
        .init();

    let config_path = &args.config;

    let quiet = args.quiet;
    let color = args.color;
    let result = match args.command {
        None => run_scan(config_path, false, false, quiet, color),
        Some(Command::Scan { json, dry_run }) => run_scan(config_path, json, dry_run, quiet, color),
        Some(Command::Add { symbol, source }) => {
            let source = source.map(|s| s.parse::<DataSource>()).transpose();
            match source {
                Ok(src) => watchlist::add(config_path, &symbol, src),
                Err(e) => Err(e),
            }
        }
        Some(Command::Remove { symbol }) => watchlist::remove(config_path, &symbol),
        Some(Command::List { json }) => watchlist::list(config_path, json),
        Some(Command::Check { telegram }) => run_check(config_path, telegram),
        Some(Command::Status { json }) => run_status(config_path, json),
        Some(Command::Completions { shell }) => {
            let mut cmd = Args::command();
            clap_complete::generate(
                shell,
                &mut cmd,
                "cdc-az-daily-alert",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    };

    if let Err(e) = result {
        log::error!("{e:#}");
        let code = if is_config_error(&e) { 2 } else { 1 };
        std::process::exit(code);
    }
}

fn is_config_error(err: &anyhow::Error) -> bool {
    let msg = format!("{err:#}");
    msg.contains("Failed to read config")
        || msg.contains("Failed to parse config")
        || msg.contains("must be set")
        || msg.contains("cannot be empty")
}

/// Executes the full scan workflow: load config, fetch candles for each symbol,
/// detect crossovers, send alerts for new signals, and persist state.
fn run_scan(
    config_path: &str,
    json: bool,
    dry_run: bool,
    quiet: bool,
    color: ColorMode,
) -> anyhow::Result<()> {
    let scan_start = Instant::now();

    let config = Config::load(config_path)?;

    if config.watchlist.is_empty() {
        log::warn!("Watchlist is empty — nothing to scan. Use 'add' to add symbols.");
        return Ok(());
    }

    log::info!(
        "Scan started: {} symbol(s) in watchlist",
        config.watchlist.len()
    );
    if dry_run {
        log::info!("DRY RUN mode — alerts will not be sent");
    }

    let mut state = StateStore::load(&config.state_file);

    let yahoo = YahooProvider::new();
    let binance = BinanceProvider::new();

    let mut signal_outputs: Vec<SignalOutput> = Vec::new();
    let mut error_messages: Vec<String> = Vec::new();
    let mut signals_found = 0;
    let mut errors = 0;
    let total = config.watchlist.len();

    let show_progress =
        !json && !quiet && !matches!(color, ColorMode::Never) && std::io::stderr().is_terminal();

    for (idx, entry) in config.watchlist.iter().enumerate() {
        if show_progress {
            eprint!("\r[{}/{}] {}...\x1b[K", idx + 1, total, entry.symbol);
        }
        let symbol_start = Instant::now();
        let provider: &dyn DataProvider = match entry.source {
            DataSource::Yahoo => &yahoo,
            DataSource::Binance => &binance,
        };

        let candles = match provider.fetch_candles(&entry.symbol) {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("{}: fetch failed: {e:#}", entry.symbol);
                log::error!("{msg}");
                errors += 1;
                error_messages.push(msg);
                continue;
            }
        };

        let Some(signal) = cdc_zone::analyze(&entry.symbol, &candles) else {
            log::debug!(
                "{}: no crossover detected (took {:.0?})",
                entry.symbol,
                symbol_start.elapsed()
            );
            continue;
        };

        if state.should_alert(&entry.symbol, &signal) {
            let alerted = if dry_run {
                log::info!(
                    "{}: DRY RUN — would send {} signal (zone: {}, price: {:.2})",
                    entry.symbol,
                    signal.signal_type,
                    signal.zone.label(),
                    signal.price
                );
                false
            } else {
                match alerts::telegram::send_alert(&config, &signal) {
                    Ok(()) => {
                        log::info!(
                            "{}: {} signal sent (zone: {}, price: {:.2})",
                            entry.symbol,
                            signal.signal_type,
                            signal.zone.label(),
                            signal.price
                        );
                        true
                    }
                    Err(e) => {
                        let msg = format!("{}: alert delivery failed: {e:#}", entry.symbol);
                        log::error!("{msg}");
                        errors += 1;
                        error_messages.push(msg);
                        false
                    }
                }
            };

            signals_found += 1;
            signal_outputs.push(SignalOutput {
                symbol: entry.symbol.clone(),
                signal: signal.signal_type.to_string(),
                zone: signal.zone.label().to_string(),
                price: signal.price,
                rsi: signal.rsi,
                volume_ratio: signal.volume_ratio,
                alerted,
            });

            state.update(&signal);
        } else {
            log::info!(
                "{}: {} signal unchanged since last alert, skipping",
                entry.symbol,
                signal.signal_type
            );
        }

        log::debug!(
            "{}: processed in {:.0?}",
            entry.symbol,
            symbol_start.elapsed()
        );
    }

    state.save()?;

    if show_progress {
        eprint!("\r\x1b[K");
    }

    let duration = scan_start.elapsed();

    if json {
        let timestamp = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string());

        let output = ScanOutput {
            timestamp,
            symbols_scanned: total,
            signals: signal_outputs,
            errors: error_messages,
            duration_ms: duration.as_millis() as u64,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        log::info!(
            "Scan complete: {total} symbols, {signals_found} new signal(s), {errors} error(s) in {duration:.1?}"
        );
        if !error_messages.is_empty() {
            log::warn!(
                "Failed symbols: {}",
                error_messages
                    .iter()
                    .map(|m| m.split(':').next().unwrap_or("?"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !quiet {
            use owo_colors::OwoColorize;
            use owo_colors::Stream::Stdout;
            println!(
                "{} {signals_found} signal(s) from {total} symbols ({duration:.1?})",
                "\u{2713}".if_supports_color(Stdout, |s| s.green())
            );
        }
    }

    if errors == total {
        anyhow::bail!(
            "All {total} symbols failed during scan. Run with -v for details, or use 'check' to validate configuration."
        );
    }
    Ok(())
}

/// Checks whether an environment variable is set and non-empty, printing the result.
fn check_env_var(name: &str) -> bool {
    use owo_colors::OwoColorize;
    use owo_colors::Stream::Stdout;

    print!("{name}... ");
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => {
            println!("{}", "set".if_supports_color(Stdout, |s| s.green()));
            true
        }
        _ => {
            println!("{}", "missing".if_supports_color(Stdout, |s| s.red()));
            false
        }
    }
}

/// Validates configuration file and optionally tests Telegram connectivity.
fn run_check(config_path: &str, test_telegram: bool) -> anyhow::Result<()> {
    use owo_colors::OwoColorize;
    use owo_colors::Stream::Stdout;

    // Check 1: Config file readable and parseable
    print!("Config file ({config_path})... ");
    match std::fs::read_to_string(config_path) {
        Ok(content) => match toml::from_str::<toml::Value>(&content) {
            Ok(_) => println!("{}", "OK".if_supports_color(Stdout, |s| s.green())),
            Err(e) => {
                println!("{}: {e}", "FAIL".if_supports_color(Stdout, |s| s.red()));
                anyhow::bail!("Config file parse error");
            }
        },
        Err(e) => {
            println!("{}: {e}", "FAIL".if_supports_color(Stdout, |s| s.red()));
            anyhow::bail!("Failed to read config file: {config_path}");
        }
    }

    // Check 2: Watchlist entries
    let entries = watchlist::load(config_path)?;
    print!("Watchlist ({} entries)... ", entries.len());
    if entries.is_empty() {
        println!("{}", "empty".if_supports_color(Stdout, |s| s.yellow()));
    } else {
        println!("{}", "OK".if_supports_color(Stdout, |s| s.green()));
    }

    // Check 3: Environment variables
    dotenvy::dotenv().ok();
    let mut has_required = true;

    has_required &= check_env_var("TELEGRAM_BOT_TOKEN");
    has_required &= check_env_var("TELEGRAM_CHAT_ID");

    // Check 4: Telegram connectivity (optional)
    if test_telegram {
        print!("Telegram API (getMe)... ");
        let token = std::env::var("TELEGRAM_BOT_TOKEN")
            .map_err(|_| anyhow::anyhow!("TELEGRAM_BOT_TOKEN must be set for --telegram check"))?;
        match alerts::telegram::ping(&token) {
            Ok(bot_name) => println!(
                "{} (bot: @{bot_name})",
                "OK".if_supports_color(Stdout, |s| s.green())
            ),
            Err(e) => {
                println!("{}: {e}", "FAIL".if_supports_color(Stdout, |s| s.red()));
                has_required = false;
            }
        }
    }

    if !has_required {
        println!();
        anyhow::bail!("One or more required checks failed");
    }

    println!(
        "\n{}",
        "All checks passed.".if_supports_color(Stdout, |s| s.green())
    );
    Ok(())
}

/// Displays the last known signal state for each tracked symbol.
fn run_status(config_path: &str, json: bool) -> anyhow::Result<()> {
    let state_file = Config::state_file_path(config_path)?;
    let state = StateStore::load(&state_file);

    if state.signals.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No signals tracked yet. Run a scan first.");
        }
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&state.signals)?);
    } else {
        use owo_colors::OwoColorize;
        use owo_colors::Stream::Stdout;

        println!(
            "{:<15} {:<8} {:<15} {:<12} {:>10}",
            "SYMBOL", "SIGNAL", "ZONE", "DATE", "PRICE"
        );
        println!("{}", "-".repeat(62));

        let mut entries: Vec<_> = state.signals.iter().collect();
        entries.sort_by_key(|(sym, _)| (*sym).clone());

        for (symbol, last) in entries {
            let signal_str = last.signal_type.to_string();
            let (indicator, colored_signal) = match last.signal_type {
                crate::signals::SignalType::Buy => (
                    "\u{25b2}",
                    signal_str
                        .if_supports_color(Stdout, |s| s.green())
                        .to_string(),
                ),
                crate::signals::SignalType::Sell => (
                    "\u{25bc}",
                    signal_str
                        .if_supports_color(Stdout, |s| s.red())
                        .to_string(),
                ),
            };
            println!(
                "{:<15} {} {:<5} {:<15} {:<12} {:>10.2}",
                symbol.if_supports_color(Stdout, |s| s.bold()),
                indicator,
                colored_signal,
                last.zone.label(),
                last.date,
                last.price
            );
        }
        println!("\n{} symbol(s) tracked.", state.signals.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_config_error_read_failure() {
        let err = anyhow::anyhow!("Failed to read config file: config.toml");
        assert!(is_config_error(&err));
    }

    #[test]
    fn test_is_config_error_parse_failure() {
        let err = anyhow::anyhow!("Failed to parse config file: config.toml");
        assert!(is_config_error(&err));
    }

    #[test]
    fn test_is_config_error_missing_env_var() {
        let err = anyhow::anyhow!("TELEGRAM_BOT_TOKEN must be set");
        assert!(is_config_error(&err));
    }

    #[test]
    fn test_is_config_error_empty_env_var() {
        let err = anyhow::anyhow!("TELEGRAM_BOT_TOKEN cannot be empty");
        assert!(is_config_error(&err));
    }

    #[test]
    fn test_is_config_error_runtime_error_returns_false() {
        let err = anyhow::anyhow!("network timeout connecting to api.binance.com");
        assert!(!is_config_error(&err));
    }
}
