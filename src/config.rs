//! Configuration loading from TOML file and environment variables.
//!
//! Non-secret settings reside in `config.toml`. Secrets (Telegram token, chat ID,
//! Discord webhook URL) are sourced exclusively from environment variables per the
//! 12-Factor App convention.
//!
//! Alert channels are selected by env-var presence: setting `TELEGRAM_BOT_TOKEN` +
//! `TELEGRAM_CHAT_ID` enables Telegram, setting `DISCORD_WEBHOOK_URL` enables
//! Discord, and setting both enables both. At least one channel is required.

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::watchlist::{self, WatchlistEntry};

#[derive(Debug, Deserialize)]
struct TomlConfig {
    settings: Option<Settings>,
}

#[derive(Debug, Deserialize)]
struct Settings {
    state_file: Option<String>,
}

/// Telegram credentials; present only when both env vars are set.
#[derive(Debug)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
}

/// Fully-resolved application configuration combining TOML values and environment secrets.
#[derive(Debug)]
pub struct Config {
    /// `Some` when `TELEGRAM_BOT_TOKEN` and `TELEGRAM_CHAT_ID` are both set.
    pub telegram: Option<TelegramConfig>,
    /// `Some` when `DISCORD_WEBHOOK_URL` is set.
    pub discord_webhook_url: Option<String>,
    pub state_file: String,
    pub watchlist: Vec<WatchlistEntry>,
}

impl Config {
    /// Extracts only the `state_file` path from config, without requiring secrets.
    ///
    /// Used by commands that read state but do not send alerts (e.g., `status`).
    /// The `STATE_FILE` env var overrides the TOML value if set.
    pub fn state_file_path(config_path: &str) -> anyhow::Result<String> {
        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config file: {config_path}"))?;
        let toml_cfg: TomlConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {config_path}"))?;
        Ok(resolve_state_file(
            toml_cfg.settings.and_then(|s| s.state_file),
        ))
    }

    /// Loads the full configuration required for scan operations.
    ///
    /// Resolution order:
    /// 1. TOML file at `path` provides `state_file` and `[[watchlist]]`
    /// 2. `.env` file (if present) populates environment variables via `dotenvy`
    /// 3. Alert channels resolve from env-var presence (Telegram and/or Discord);
    ///    at least one channel must be configured
    /// 4. `STATE_FILE` env var overrides the TOML value for deployment flexibility
    ///
    /// # Errors
    ///
    /// Returns an error if the TOML file is unreadable/unparseable, Telegram
    /// credentials are only partially set, or no alert channel is configured.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {path}"))?;
        let toml_cfg: TomlConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {path}"))?;

        let telegram_bot_token = env_non_empty("TELEGRAM_BOT_TOKEN");
        let telegram_chat_id = env_non_empty("TELEGRAM_CHAT_ID");
        let discord_webhook_url = env_non_empty("DISCORD_WEBHOOK_URL");

        let telegram = match (telegram_bot_token, telegram_chat_id) {
            (Some(bot_token), Some(chat_id)) => Some(TelegramConfig { bot_token, chat_id }),
            (None, None) => None,
            _ => bail!("TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID must be set together"),
        };

        if telegram.is_none() && discord_webhook_url.is_none() {
            bail!(
                "No alert channel configured: TELEGRAM_BOT_TOKEN + TELEGRAM_CHAT_ID \
                 and/or DISCORD_WEBHOOK_URL must be set"
            );
        }

        let state_file = resolve_state_file(toml_cfg.settings.and_then(|s| s.state_file));

        let watchlist = watchlist::load(path)?;

        log::debug!(
            "Config loaded: state_file={state_file}, watchlist={} symbols, channels={}",
            watchlist.len(),
            [
                telegram.as_ref().map(|_| "telegram"),
                discord_webhook_url.as_ref().map(|_| "discord"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("+")
        );

        Ok(Self {
            telegram,
            discord_webhook_url,
            state_file,
            watchlist,
        })
    }
}

/// Reads an environment variable, treating empty strings as unset.
fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Resolves the state file path: the `STATE_FILE` env var overrides the TOML
/// value; falls back to `last_signals.json`.
fn resolve_state_file(toml_value: Option<String>) -> String {
    std::env::var("STATE_FILE")
        .unwrap_or_else(|_| toml_value.unwrap_or_else(|| "last_signals.json".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_config(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn test_state_file_path_reads_from_toml() {
        let f = write_config(
            r#"
[settings]
state_file = "custom_state.json"
"#,
        );
        let result = Config::state_file_path(f.path().to_str().unwrap()).unwrap();
        assert_eq!(result, "custom_state.json");
    }

    #[test]
    fn test_state_file_path_default_when_missing() {
        let f = write_config("[settings]\n");
        let result = Config::state_file_path(f.path().to_str().unwrap()).unwrap();
        assert_eq!(result, "last_signals.json");
    }

    #[test]
    fn test_state_file_path_default_no_settings_section() {
        let f = write_config("[[watchlist]]\nsymbol = \"BTC-USD\"\n");
        let result = Config::state_file_path(f.path().to_str().unwrap()).unwrap();
        assert_eq!(result, "last_signals.json");
    }

    #[test]
    fn test_state_file_path_file_not_found() {
        let result = Config::state_file_path("/nonexistent/path/config.toml");
        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("Failed to read config"));
    }

    #[test]
    fn test_state_file_path_invalid_toml() {
        let f = write_config("{{{{ not valid toml");
        let result = Config::state_file_path(f.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("Failed to parse config"));
    }

    #[test]
    fn test_load_missing_file_returns_error() {
        let result = Config::load("/nonexistent/config.toml");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_invalid_toml_returns_error() {
        let f = write_config("not valid {{ toml");
        let result = Config::load(f.path().to_str().unwrap());
        assert!(result.is_err());
    }

    // Env-var-dependent tests: these exercise Config::load(), which reads
    // channel credentials from the process environment. cargo test runs
    // tests in parallel within one process, so env var mutations race.
    // These tests are marked #[ignore] for the default parallel run and
    // execute separately with --test-threads=1.

    fn clear_channel_env() {
        unsafe {
            std::env::remove_var("TELEGRAM_BOT_TOKEN");
            std::env::remove_var("TELEGRAM_CHAT_ID");
            std::env::remove_var("DISCORD_WEBHOOK_URL");
        }
    }

    #[test]
    #[ignore = "mutates process env vars, must run single-threaded"]
    fn test_load_no_channel_returns_error() {
        clear_channel_env();

        let f = write_config("[settings]\nstate_file = \"test.json\"\n");
        let result = Config::load(f.path().to_str().unwrap());

        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("No alert channel configured"));
    }

    #[test]
    #[ignore = "mutates process env vars, must run single-threaded"]
    fn test_load_partial_telegram_returns_error() {
        clear_channel_env();
        unsafe {
            // Empty string counts as unset, so this is token-only.
            std::env::set_var("TELEGRAM_BOT_TOKEN", "");
            std::env::set_var("TELEGRAM_CHAT_ID", "12345");
        }

        let f = write_config("[settings]\nstate_file = \"test.json\"\n");
        let result = Config::load(f.path().to_str().unwrap());

        clear_channel_env();

        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("must be set together"));
    }

    #[test]
    #[ignore = "mutates process env vars, must run single-threaded"]
    fn test_load_discord_only_is_valid() {
        clear_channel_env();
        unsafe {
            std::env::set_var(
                "DISCORD_WEBHOOK_URL",
                "https://discord.com/api/webhooks/1/fake",
            );
        }

        let f = write_config("[settings]\nstate_file = \"test.json\"\n");
        let result = Config::load(f.path().to_str().unwrap());

        clear_channel_env();

        let config = result.unwrap();
        assert!(config.telegram.is_none());
        assert_eq!(
            config.discord_webhook_url.as_deref(),
            Some("https://discord.com/api/webhooks/1/fake")
        );
    }

    #[test]
    #[ignore = "mutates process env vars, must run single-threaded"]
    fn test_load_valid_config() {
        clear_channel_env();
        unsafe {
            std::env::set_var("TELEGRAM_BOT_TOKEN", "fake_token_for_valid_test");
            std::env::set_var("TELEGRAM_CHAT_ID", "99999");
        }

        let f = write_config(
            r#"
[settings]
state_file = "test_state.json"

[[watchlist]]
symbol = "BTC-USD"
source = "binance"
"#,
        );

        let config = Config::load(f.path().to_str().unwrap()).unwrap();

        clear_channel_env();

        let telegram = config.telegram.expect("telegram channel should be active");
        assert_eq!(telegram.bot_token, "fake_token_for_valid_test");
        assert_eq!(telegram.chat_id, "99999");
        assert!(config.discord_webhook_url.is_none());
        assert_eq!(config.state_file, "test_state.json");
        assert_eq!(config.watchlist.len(), 1);
        assert_eq!(config.watchlist[0].symbol, "BTC-USD");
    }
}
