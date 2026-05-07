# CDC Action Zone Monitor

## Overview

A lightweight Rust CLI that scans a watchlist of assets daily for CDC Action Zone
(EMA 12/26 crossover) signals and sends Telegram alerts. The binary runs via cron
on a Raspberry Pi or $5/mo VPS.

## Architecture

Single binary, no daemon. Cron triggers → fetch prices → detect crossovers → alert → exit.

```
main.rs → config + watchlist + state
        → for each symbol: data provider → cdc_zone::analyze → telegram alert
```

## Key Decisions

- **EMA(12)/EMA(26)**: Original CDC Action Zone parameters (piriya33, TradingView)
- **ureq** (not reqwest): True blocking HTTP, no tokio runtime, smaller binary
- **time** (not chrono): Lighter, no soundness issues, UTC-only is sufficient
- **clap v4** (derive): Industry standard CLI framework — shell completions, colored help, typo suggestions
- **owo-colors**: Zero-cost terminal coloring with automatic `NO_COLOR`/TTY detection
- **prek**: Single-binary pre-commit hook runner (no Python dependency)
- **Pedantic clippy**: Enabled via `[lints.clippy]` in Cargo.toml with targeted allows
- Alerts fire on **crossover events only**; RSI/volume are informational context

## Module Layout

```
src/
├── main.rs            CLI dispatch + scan orchestration
├── config.rs          TOML + env var config loading
├── watchlist.rs       TOML-backed watchlist CRUD
├── state.rs           Last signal tracking (JSON)
├── data.rs            Candle type + DataProvider trait
├── data/
│   ├── yahoo.rs       Yahoo Finance chart API
│   └── binance.rs     Binance klines API
├── signals.rs         Signal/Zone types
├── signals/
│   ├── indicators.rs  EMA, SMA, RSI, volume_ratio
│   └── cdc_zone.rs    Crossover detection
├── alerts.rs          Module declarations
└── alerts/
    └── telegram.rs    Telegram Bot API (sendMessage + getMe)
```

## Commands

```bash
cargo build --release                    # Build
cargo test                               # Run unit tests
cargo run -- scan                        # Run scan (needs .env for secrets)
cargo run -- scan --dry-run              # Analyze without sending alerts
cargo run -- scan --json                 # JSON output to stdout
cargo run -- list                        # Show watchlist (colored table)
cargo run -- list --json                 # Watchlist as JSON
cargo run -- add SOL-USD                 # Add symbol (auto-detects source)
cargo run -- add MSFT --source yahoo     # Add with explicit source
cargo run -- remove AAPL                 # Remove symbol
cargo run -- check                       # Validate config + env vars
cargo run -- check --telegram            # Also ping Telegram API
cargo run -- status                      # Show last signals from state file
cargo run -- completions bash            # Generate shell completions
cargo run -- --config custom.toml list   # Use alternate config file
cargo run -- --color never list           # Force no color
cargo run -- --color always list         # Force color (even in pipes)
cargo run -- -v scan                     # Debug verbosity
cargo run -- -q scan                     # Quiet mode (errors only)
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Runtime error (network failure, all symbols failed) |
| 2 | Configuration error (missing file, invalid TOML, missing env vars) |

## Configuration

Non-secret settings and the watchlist reside in `config.toml`. Secrets are in `.env` (gitignored):

**`config.toml`** (committed):
- `[settings]` — `state_file` path
- `[[watchlist]]` — symbol entries with optional `source` field

**`.env`** (secrets only, copy from `.env.example`):
- `TELEGRAM_BOT_TOKEN` (required)
- `TELEGRAM_CHAT_ID` (required)
- `STATE_FILE` (optional — overrides `state_file` from config.toml)

## Data Sources

- Symbols with `-USD` and no `.` → Binance (e.g., `BTC-USD`, `ETH-USD`)
- All others → Yahoo Finance (e.g., `AAPL`, `PTT.BK`)

## Pre-commit Hooks

Managed by [prek](https://github.com/j178/prek) — a single-binary, zero-dependency pre-commit hook runner.

```bash
brew install prek            # Install (or: cargo binstall prek)
prek install                 # Wire up .git/hooks/pre-commit
prek run --all-files         # Run all hooks manually
```

Hooks (defined in `.pre-commit-config.yaml`):
1. `cargo fmt --` — formatting check
2. `cargo clippy --all-targets -- -D warnings` — lint (warnings = errors)
3. `cargo test` — unit tests

## Code Quality

- **Formatter**: `cargo fmt` (config: `.rustfmt.toml`, edition 2024)
- **Linter**: `cargo clippy` with pedantic warnings (config: `[lints.clippy]` in `Cargo.toml`)
- **Pre-commit**: prek runs fmt → clippy → test before each commit (config: `.pre-commit-config.yaml`)

```bash
cargo fmt -- --check                     # Verify formatting
cargo clippy --all-targets -- -D warnings  # Lint with warnings as errors
prek run --all-files                     # Run all pre-commit hooks
```

## Testing

```bash
cargo test                  # Unit tests (no network)
cargo run -- -v scan        # Debug verbosity
cargo run -- -vv scan       # Trace verbosity (HTTP URLs)
```

## Deployment

```bash
cargo build --release
scp target/release/cdc-az-daily-alert server:/opt/cdc-monitor/
scp config.toml server:/opt/cdc-monitor/
scp .env server:/opt/cdc-monitor/
# Cron: 0 22 * * * cd /opt/cdc-monitor && ./cdc-az-daily-alert -q scan
```
