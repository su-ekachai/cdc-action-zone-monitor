# Getting Started

## Prerequisites

- Rust toolchain (1.85+): https://rustup.rs
- A Telegram account and/or a Discord server (alerts go to either or both)

## Step 1: Create a Telegram Bot

Skip to Step 2b if you only want Discord alerts.

1. Open Telegram and message [@BotFather](https://t.me/BotFather)
2. Send `/newbot`
3. Choose a name (e.g., "CDC Monitor") and username (e.g., "my_cdc_bot")
4. Copy the **bot token** (format: `123456789:ABCdefGHI...`)

## Step 2: Get the Chat ID

1. Message [@userinfobot](https://t.me/userinfobot) on Telegram
2. The reply contains the **chat ID** (a number like `123456789`)
3. For group delivery: add the bot to the group, then use the group's chat ID (prefix `-100`)

## Step 2b: Create a Discord Webhook (Optional)

1. In your Discord server: **Server Settings → Integrations → Webhooks → New Webhook**
2. Pick the target channel and copy the **webhook URL** (format: `https://discord.com/api/webhooks/...`)

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

Edit `.env` (secrets only — not committed to git). A channel is enabled by
setting its variables; configure Telegram, Discord, or both — at least one
is required:
```
TELEGRAM_BOT_TOKEN=123456789:ABCdefGHI...
TELEGRAM_CHAT_ID=123456789
DISCORD_WEBHOOK_URL=https://discord.com/api/webhooks/...
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
# Verify config file, env vars, and optionally channel connectivity
./target/release/cdc-az-daily-alert check --telegram --discord
```

Expected output when all checks pass (Telegram + Discord configured):
```
Config file (config.toml)... OK
Watchlist (3 entries)... OK
TELEGRAM_BOT_TOKEN... set
TELEGRAM_CHAT_ID... set
DISCORD_WEBHOOK_URL... set
Alert channels... OK (telegram + discord)
Telegram API (getMe)... OK (bot: @my_cdc_bot)
Discord webhook (GET)... OK (webhook: CDC Monitor)

All checks passed.
```

Only pass `--telegram`/`--discord` for channels you configured.

## Step 7: Test a Scan

```bash
# Dry run (fetches data, detects signals, does NOT send alerts)
./target/release/cdc-az-daily-alert scan --dry-run

# Full scan (sends Telegram alerts for new signals)
./target/release/cdc-az-daily-alert scan
```

## Step 8: Schedule Daily Runs

Pick **one** runner — GitHub Actions or your own machine. Running both
duplicates alerts and lets their state files diverge.

### GitHub Actions (zero-cost, no server needed)

The repo ships with `.github/workflows/daily-scan.yml`, which builds and runs
the scan daily at 00:15 UTC (just after the Binance daily candle closes) and
keeps `last_signals.json` in the Actions cache between runs.

1. Push the repo to GitHub (public repos get unlimited Actions minutes)
2. Add repo secrets under **Settings → Secrets and variables → Actions**:
   - `TELEGRAM_BOT_TOKEN` and `TELEGRAM_CHAT_ID` (for Telegram)
   - `DISCORD_WEBHOOK_URL` (for Discord)
3. Test it: **Actions → Daily Scan → Run workflow**

Known limitations:
- GitHub cron is best-effort — runs may start 5–30 minutes late (fine for daily candles)
- Scheduled workflows auto-disable after 60 days without repo activity; the
  workflow's keepalive step prevents this by re-enabling itself on every run
- Signal state lives in the Actions cache, which GitHub evicts after 7 days
  without use — after a longer outage the next scan starts from empty state
- Yahoo Finance occasionally rate-limits datacenter IPs; the workflow retries
  once after 5 minutes, and persistent failures are reported to your alert channels
- On days with no signals, a short heartbeat message confirms the scan ran

### Linux/Mac (cron)

```bash
crontab -e
```

Add this line (runs at 00:05 UTC daily, just after the Binance daily candle closes):

```
5 0 * * * cd /path/to/cdc-action-zone-monitor && ./target/release/cdc-az-daily-alert -q scan >> scan.log 2>&1
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

**"No alert channel configured"**
→ Ensure `.env` exists in the directory where the binary runs and sets
`TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID` and/or `DISCORD_WEBHOOK_URL`.
Run `check` to verify.

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
