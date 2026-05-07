//! Configuration loading from TOML file and environment variables.
//!
//! Non-secret settings reside in `config.toml`. Secrets (Telegram token, chat ID)
//! are sourced exclusively from environment variables per the 12-Factor App convention.

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

/// Fully-resolved application configuration combining TOML values and environment secrets.
#[derive(Debug)]
pub struct Config {
    pub telegram_bot_token: String,
    pub telegram_chat_id: String,
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
        Ok(std::env::var("STATE_FILE").unwrap_or_else(|_| {
            toml_cfg
                .settings
                .and_then(|s| s.state_file)
                .unwrap_or_else(|| "last_signals.json".to_string())
        }))
    }

    /// Loads the full configuration required for scan operations.
    ///
    /// Resolution order:
    /// 1. TOML file at `path` provides `state_file` and `[[watchlist]]`
    /// 2. `.env` file (if present) populates environment variables via `dotenvy`
    /// 3. `TELEGRAM_BOT_TOKEN` and `TELEGRAM_CHAT_ID` are required from the environment
    /// 4. `STATE_FILE` env var overrides the TOML value for deployment flexibility
    ///
    /// # Errors
    ///
    /// Returns an error if the TOML file is unreadable/unparseable or required
    /// environment variables are missing or empty.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {path}"))?;
        let toml_cfg: TomlConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {path}"))?;

        let telegram_bot_token =
            std::env::var("TELEGRAM_BOT_TOKEN").context("TELEGRAM_BOT_TOKEN must be set")?;
        let telegram_chat_id =
            std::env::var("TELEGRAM_CHAT_ID").context("TELEGRAM_CHAT_ID must be set")?;

        if telegram_bot_token.is_empty() {
            bail!("TELEGRAM_BOT_TOKEN cannot be empty");
        }
        if telegram_chat_id.is_empty() {
            bail!("TELEGRAM_CHAT_ID cannot be empty");
        }

        let settings = toml_cfg.settings.unwrap_or(Settings { state_file: None });

        let state_file = std::env::var("STATE_FILE").unwrap_or_else(|_| {
            settings
                .state_file
                .unwrap_or_else(|| "last_signals.json".to_string())
        });

        let watchlist = watchlist::load(path)?;

        log::debug!(
            "Config loaded: state_file={state_file}, watchlist={} symbols",
            watchlist.len()
        );

        Ok(Self {
            telegram_bot_token,
            telegram_chat_id,
            state_file,
            watchlist,
        })
    }
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

    // Env-var-dependent tests: These test Config::load() which reads
    // TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID from the process environment.
    // Because cargo test runs tests in parallel within the same process,
    // env var mutations can race. We mark these #[ignore] for the default
    // parallel run and execute them separately with --test-threads=1.

    #[test]
    #[ignore = "mutates process env vars, must run single-threaded"]
    fn test_load_missing_telegram_token_returns_error() {
        unsafe {
            std::env::remove_var("TELEGRAM_BOT_TOKEN");
            std::env::remove_var("TELEGRAM_CHAT_ID");
        }

        let f = write_config("[settings]\nstate_file = \"test.json\"\n");
        let result = Config::load(f.path().to_str().unwrap());

        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("TELEGRAM_BOT_TOKEN"));
    }

    #[test]
    #[ignore = "mutates process env vars, must run single-threaded"]
    fn test_load_empty_telegram_token_returns_error() {
        unsafe {
            std::env::set_var("TELEGRAM_BOT_TOKEN", "");
            std::env::set_var("TELEGRAM_CHAT_ID", "12345");
        }

        let f = write_config("[settings]\nstate_file = \"test.json\"\n");
        let result = Config::load(f.path().to_str().unwrap());

        unsafe {
            std::env::remove_var("TELEGRAM_BOT_TOKEN");
            std::env::remove_var("TELEGRAM_CHAT_ID");
        }

        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("cannot be empty"));
    }

    #[test]
    #[ignore = "mutates process env vars, must run single-threaded"]
    fn test_load_valid_config() {
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

        unsafe {
            std::env::remove_var("TELEGRAM_BOT_TOKEN");
            std::env::remove_var("TELEGRAM_CHAT_ID");
        }

        assert_eq!(config.telegram_bot_token, "fake_token_for_valid_test");
        assert_eq!(config.telegram_chat_id, "99999");
        assert_eq!(config.state_file, "test_state.json");
        assert_eq!(config.watchlist.len(), 1);
        assert_eq!(config.watchlist[0].symbol, "BTC-USD");
    }
}
