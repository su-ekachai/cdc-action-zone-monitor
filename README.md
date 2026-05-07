# CDC Action Zone Monitor

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg)](https://rustup.rs)

A lightweight Rust CLI that scans a watchlist of assets daily for [CDC Action Zone](https://www.tradingview.com/script/gBBKMr2T-CDC-ActionZone-V3-2020/) buy/sell signals and delivers Telegram alerts. Runs in under 5 seconds, uses less than 5 MB RAM, and deploys as a single binary with no runtime dependencies.

## Demo

```
$ cdc-az-daily-alert scan --dry-run
[1/5] BTC-USD...
[2/5] ETH-USD...
[3/5] TSLA...
[4/5] AAPL...
[5/5] SOL-USD...
✓ 2 signal(s) from 5 symbols (3.2s)
```

Telegram alert delivered on signal detection:

```
🟢 Buy Signal: TSLA
Price: 245.67
Zone: Bull

Strength:
• RSI(14): 62.3
• Volume: 1.8x average
• Trend: Bullish (above SMA50)

EMA(12): 243.50 crossed above EMA(26): 241.20
```

## Features

- Detects EMA(12)/EMA(26) crossover signals (original CDC Action Zone strategy)
- Supports stocks (Yahoo Finance) and crypto (Binance)
- Sends formatted Telegram alerts with RSI, volume, and trend context
- Tracks state to avoid duplicate alerts (only fires on direction change)
- Accessible CLI output with directional indicators (▲/▼) alongside color
- JSON output mode for scripting and pipeline integration
- Shell completions (bash, zsh, fish)
- Single binary, no runtime dependencies

## Installation

Requires [Rust 1.85+](https://rustup.rs).

```bash
git clone https://github.com/su-ekachai/cdc-action-zone-monitor.git
cd cdc-action-zone-monitor
cargo build --release
```

The binary is at `target/release/cdc-az-daily-alert`.

## Quick Start

```bash
# 1. Configure secrets
cp .env.example .env
# Edit .env with your Telegram bot token and chat ID

# 2. Add symbols to watchlist
./target/release/cdc-az-daily-alert add TSLA
./target/release/cdc-az-daily-alert add BTC-USD

# 3. Validate setup
./target/release/cdc-az-daily-alert check --telegram

# 4. Run a scan
./target/release/cdc-az-daily-alert scan --dry-run   # Preview (no alerts sent)
./target/release/cdc-az-daily-alert scan             # Live (sends Telegram alerts)
```

For detailed setup instructions (Telegram bot creation, chat ID, cron scheduling), see [`docs/getting-started.md`](docs/getting-started.md).

## Usage

```
cdc-az-daily-alert [OPTIONS] [COMMAND]

Commands:
  scan         Scan watchlist for signals and send alerts [default]
  add          Add a symbol to the watchlist
  remove       Remove a symbol from the watchlist
  list         List all symbols in the watchlist
  check        Validate configuration and connectivity
  status       Show last scan results from state file
  completions  Generate shell completions

Options:
  -c, --config <FILE>   Path to config file [default: config.toml]
  -v, --verbose...      Increase verbosity (-v = debug, -vv = trace)
  -q, --quiet           Suppress non-error output
      --color <MODE>    Color output: auto, always, never [default: auto]
  -V, --version         Print version
```

### Watchlist Management

```bash
cdc-az-daily-alert list                    # Show watchlist (colored table)
cdc-az-daily-alert list --json             # JSON output
cdc-az-daily-alert add SOL-USD             # Auto-detects Binance (crypto)
cdc-az-daily-alert add MSFT --source yahoo # Explicit source
cdc-az-daily-alert remove PTT.BK           # Remove symbol
```

### Scanning

```bash
cdc-az-daily-alert scan                    # Full scan (sends alerts)
cdc-az-daily-alert scan --dry-run          # Analyze without sending alerts
cdc-az-daily-alert scan --json             # Structured JSON output
cdc-az-daily-alert -q scan                 # Quiet mode (for cron)
```

### Operational Commands

```bash
cdc-az-daily-alert check                   # Validate config and env vars
cdc-az-daily-alert check --telegram        # Also verify Telegram API connectivity
cdc-az-daily-alert status                  # Show last signals per symbol
cdc-az-daily-alert status --json           # JSON format
cdc-az-daily-alert completions bash        # Generate shell completions
```

## Configuration

Settings and watchlist reside in `config.toml` (committed). Secrets are environment variables loaded from `.env` (gitignored).

**`config.toml`**

```toml
[settings]
state_file = "last_signals.json"

[[watchlist]]
symbol = "BTC-USD"

[[watchlist]]
symbol = "AAPL"
source = "yahoo"
```

Source is auto-detected if omitted: `*-USD` → Binance, all others → Yahoo Finance.

**`.env`** (secrets only — copy from `.env.example`)

| Variable | Required | Description |
|----------|----------|-------------|
| `TELEGRAM_BOT_TOKEN` | Yes | Bot token from [@BotFather](https://t.me/BotFather) |
| `TELEGRAM_CHAT_ID` | Yes | Target chat/group ID |

## Signal Logic

The [CDC Action Zone](https://www.tradingview.com/script/gBBKMr2T-CDC-ActionZone-V3-2020/) strategy fires alerts on EMA crossovers:

| Signal | Condition |
|--------|-----------|
| **▲ BUY** | EMA(12) crosses above EMA(26) |
| **▼ SELL** | EMA(12) crosses below EMA(26) |

Each alert includes contextual indicators (informational, not used for signal gating):
- **RSI(14)** — momentum
- **Volume ratio** — current vs. 20-day average
- **Trend** — price vs. SMA(50)

Alerts fire once per direction change. The state file (`last_signals.json`) prevents duplicate notifications.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Scan completed successfully |
| 1 | Runtime error (network failure, all symbols failed) |
| 2 | Configuration error (missing file, invalid TOML, missing env vars) |

## Deployment

```bash
# Build optimized binary (~4 MB stripped)
cargo build --release

# Deploy to server
scp target/release/cdc-az-daily-alert user@server:/opt/cdc-monitor/
scp config.toml .env user@server:/opt/cdc-monitor/
```

Schedule daily via cron (runs at 22:00 UTC, after US market close):

```
0 22 * * * cd /opt/cdc-monitor && ./cdc-az-daily-alert -q scan >> /var/log/cdc-monitor.log 2>&1
```

### Raspberry Pi (cross-compile)

```bash
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
```

## Development

```bash
cargo test                    # Run unit tests (no network required)
cargo run -- -v scan          # Debug verbosity
cargo run -- -vv scan         # Trace verbosity (shows HTTP URLs)
cargo run -- scan --dry-run   # Test without sending alerts
```

### Code Quality

- **Formatter**: `cargo fmt` (edition 2024, config: `.rustfmt.toml`)
- **Linter**: `cargo clippy` with pedantic warnings (config: `[lints.clippy]` in `Cargo.toml`)
- **Pre-commit hooks** via [prek](https://github.com/j178/prek):

```bash
brew install prek             # Install (or: cargo binstall prek)
prek install                  # Wire up git hook
prek run --all-files          # Run all checks manually
```

Hooks run in order: `cargo fmt` → `cargo clippy --all-targets -D warnings` → `cargo test`

## License

[MIT](LICENSE)
