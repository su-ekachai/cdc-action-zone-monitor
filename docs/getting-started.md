# Getting Started

## Prerequisites

- Rust toolchain (1.85+): https://rustup.rs
- A Telegram account

## Step 1: Create a Telegram Bot

1. Open Telegram and message [@BotFather](https://t.me/BotFather)
2. Send `/newbot`
3. Choose a name (e.g., "CDC Monitor") and username (e.g., "my_cdc_bot")
4. Copy the **bot token** (format: `123456789:ABCdefGHI...`)

## Step 2: Get the Chat ID

1. Message [@userinfobot](https://t.me/userinfobot) on Telegram
2. The reply contains the **chat ID** (a number like `123456789`)
3. For group delivery: add the bot to the group, then use the group's chat ID (prefix `-100`)

## Step 3: Build the Project

```bash
cd cdc-action-zone-monitor
cargo build --release
```

The binary is at `target/release/cdc-az-daily-alert`.

## Step 4: Configure Secrets

```bash
cp .env.example .env
```

Edit `.env` (secrets only — not committed to git):
```
TELEGRAM_BOT_TOKEN=123456789:ABCdefGHI...
TELEGRAM_CHAT_ID=123456789
```

Optional: override the state file path (default reads from `config.toml`):
```
STATE_FILE=/var/lib/cdc-monitor/last_signals.json
```

## Step 5: Set Up the Watchlist

The watchlist is defined in `config.toml`. Use CLI commands to manage it:

```bash
# View current watchlist
./target/release/cdc-az-daily-alert list

# Add symbols
./target/release/cdc-az-daily-alert add TSLA
./target/release/cdc-az-daily-alert add SOL-USD

# Remove symbols
./target/release/cdc-az-daily-alert remove PTT.BK
```

Source detection is automatic:
- Symbols with `-USD` → Binance (crypto)
- All others → Yahoo Finance (stocks)

Direct editing of `config.toml` is also supported:
```toml
[[watchlist]]
symbol = "TSLA"
source = "yahoo"
```

## Step 6: Validate Configuration

```bash
# Verify config file, env vars, and optionally Telegram connectivity
./target/release/cdc-az-daily-alert check --telegram
```

Expected output when all checks pass:
```
Config file (config.toml)... OK
Watchlist (5 entries)... OK
TELEGRAM_BOT_TOKEN... set
TELEGRAM_CHAT_ID... set
Telegram API (getMe)... OK (bot: @my_cdc_bot)

All checks passed.
```

## Step 7: Test a Scan

```bash
# Dry run (fetches data, detects signals, does NOT send alerts)
./target/release/cdc-az-daily-alert scan --dry-run

# Full scan (sends Telegram alerts for new signals)
./target/release/cdc-az-daily-alert scan
```

## Step 8: Schedule Daily Runs

### Linux/Mac (cron)

```bash
crontab -e
```

Add this line (runs at 22:00 UTC daily, after US market close):

```
0 22 * * * cd /path/to/cdc-action-zone-monitor && ./target/release/cdc-az-daily-alert -q scan >> scan.log 2>&1
```

The `-q` flag suppresses informational output, logging only errors.

### Raspberry Pi

Same as above. The binary cross-compiles cleanly:

```bash
# On the dev machine (targeting arm64 Pi)
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
```

## Step 9: Set Up Pre-commit Hooks (Recommended)

Install [prek](https://github.com/j178/prek) to run code quality checks before each commit:

```bash
brew install prek
prek install
```

This wires up `.git/hooks/pre-commit` to automatically run formatting, linting, and tests before each commit. Verify with:

```bash
prek run --all-files
```

## Step 10: Install Shell Completions (Optional)

```bash
# Bash
cdc-az-daily-alert completions bash > ~/.bash_completion.d/cdc-az-daily-alert

# Zsh
cdc-az-daily-alert completions zsh > ~/.zsh/completions/_cdc-az-daily-alert

# Fish
cdc-az-daily-alert completions fish > ~/.config/fish/completions/cdc-az-daily-alert.fish
```

## Troubleshooting

**"`TELEGRAM_BOT_TOKEN` must be set"**
→ Ensure `.env` exists in the directory where the binary runs. Run `check` to verify.

**"Failed to read config file: config.toml"**
→ Run from the directory containing `config.toml`, or use `--config path/to/config.toml`.

**No alerts received**
→ Run with `-v` to see debug output:
```bash
./target/release/cdc-az-daily-alert -v scan
```

**Same signal not re-alerting**
→ By design. The state file tracks the last signal direction per symbol. Delete `last_signals.json` to reset state and re-trigger alerts.

**Checking last known signals**
→ Use the `status` command:
```bash
./target/release/cdc-az-daily-alert status
```
