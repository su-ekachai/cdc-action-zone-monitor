//! Persistent state tracking to prevent duplicate alerts.
//!
//! Stores the last emitted signal per symbol in a JSON file. On each scan,
//! [`StateStore::should_alert`] compares the new signal direction against stored
//! state to determine whether a direction change occurred (Buy→Sell or Sell→Buy).

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::signals::{Signal, SignalType, Zone};

/// The last signal emitted for a single symbol, persisted between runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastSignal {
    pub signal_type: SignalType,
    pub zone: Zone,
    /// ISO 8601 date (`YYYY-MM-DD`) of the candle that triggered the signal.
    pub date: String,
    pub price: f64,
}

/// JSON-backed store mapping symbols to their last alerted signal.
///
/// Loaded at scan start and saved after processing all symbols.
/// The file is created on first write if it does not exist.
pub struct StateStore {
    path: String,
    pub signals: HashMap<String, LastSignal>,
}

impl StateStore {
    /// Loads state from disk. Returns an empty store if the file is missing or corrupt.
    ///
    /// A missing file is expected on first run — all detected signals will alert.
    /// A corrupt file triggers a warning and resets to empty state.
    pub fn load(path: &str) -> Self {
        let signals = if Path::new(path).exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(parsed) => {
                        let map: HashMap<String, LastSignal> = parsed;
                        log::debug!("State loaded: {} tracked symbol(s) from {path}", map.len());
                        map
                    }
                    Err(e) => {
                        log::warn!(
                            "State file {path} contains invalid JSON: {e} — resetting to empty"
                        );
                        HashMap::new()
                    }
                },
                Err(e) => {
                    log::warn!("Failed to read state file {path}: {e} — resetting to empty");
                    HashMap::new()
                }
            }
        } else {
            log::info!("No state file at {path} — first run, all signals will alert");
            HashMap::new()
        };

        Self {
            path: path.to_string(),
            signals,
        }
    }

    /// Returns `true` if this signal is new since the last alert for this symbol:
    /// either the direction changed, or it is a fresh crossover on a different
    /// candle date (covers an opposite crossover missed during downtime).
    ///
    /// Also returns `true` if no previous signal exists (first detection).
    /// Same-day re-runs of an already-alerted signal return `false`.
    pub fn should_alert(&self, symbol: &str, new_signal: &Signal) -> bool {
        match self.signals.get(symbol) {
            None => true,
            Some(last) => {
                last.signal_type != new_signal.signal_type
                    || last.date != crate::data::format_date(new_signal.timestamp)
            }
        }
    }

    /// Records a signal as the new last-alerted state for its symbol.
    pub fn update(&mut self, signal: &Signal) {
        let date = crate::data::format_date(signal.timestamp);

        self.signals.insert(
            signal.symbol.clone(),
            LastSignal {
                signal_type: signal.signal_type,
                zone: signal.zone,
                date,
                price: signal.price,
            },
        );
    }

    /// Persists current state to the configured JSON file path.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or file I/O fails.
    pub fn save(&self) -> anyhow::Result<()> {
        let json =
            serde_json::to_string_pretty(&self.signals).context("Failed to serialize state")?;
        crate::fsutil::write_atomic(&self.path, &json)
            .with_context(|| format!("Failed to write state to {}", self.path))?;
        log::debug!(
            "State saved: {} symbol(s) to {}",
            self.signals.len(),
            self.path
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::{Signal, SignalType, Zone};

    fn make_signal(signal_type: SignalType) -> Signal {
        Signal {
            symbol: "TEST".to_string(),
            signal_type,
            zone: Zone::Bull,
            price: 100.0,
            fast_ema: 101.0,
            slow_ema: 99.0,
            rsi: 55.0,
            volume_ratio: 1.2,
            trend_sma50: 98.0,
            timestamp: 1_704_067_200,
        }
    }

    #[test]
    fn test_should_alert_no_previous() {
        let store = StateStore {
            path: String::new(),
            signals: HashMap::new(),
        };
        let signal = make_signal(SignalType::Buy);
        assert!(store.should_alert("TEST", &signal));
    }

    #[test]
    fn test_should_alert_same_signal() {
        let mut store = StateStore {
            path: String::new(),
            signals: HashMap::new(),
        };
        let signal = make_signal(SignalType::Buy);
        store.update(&signal);
        assert!(!store.should_alert("TEST", &signal));
    }

    #[test]
    fn test_should_alert_same_direction_new_date() {
        let mut store = StateStore {
            path: String::new(),
            signals: HashMap::new(),
        };
        let buy = make_signal(SignalType::Buy);
        store.update(&buy);

        // A fresh crossover in the same direction on a later candle date must
        // alert (covers an opposite crossover missed during downtime).
        let mut later_buy = make_signal(SignalType::Buy);
        later_buy.timestamp += 86_400;
        assert!(store.should_alert("TEST", &later_buy));
    }

    #[test]
    fn test_should_alert_different_signal() {
        let mut store = StateStore {
            path: String::new(),
            signals: HashMap::new(),
        };
        let buy = make_signal(SignalType::Buy);
        store.update(&buy);
        let sell = make_signal(SignalType::Sell);
        assert!(store.should_alert("TEST", &sell));
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let path_str = path.to_str().unwrap();

        let mut store = StateStore {
            path: path_str.to_string(),
            signals: HashMap::new(),
        };

        let signal = make_signal(SignalType::Buy);
        store.update(&signal);
        store.save().unwrap();

        let loaded = StateStore::load(path_str);
        assert_eq!(loaded.signals.len(), 1);
        assert_eq!(loaded.signals["TEST"].signal_type, SignalType::Buy);
        assert!((loaded.signals["TEST"].price - 100.0).abs() < 1e-10);
    }

    #[test]
    fn test_load_missing_file_returns_empty() {
        let store = StateStore::load("/nonexistent/path/state.json");
        assert!(store.signals.is_empty());
    }

    #[test]
    fn test_load_corrupt_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.json");
        std::fs::write(&path, "not valid json {{}}").unwrap();

        let store = StateStore::load(path.to_str().unwrap());
        assert!(store.signals.is_empty());
    }

    #[test]
    fn test_update_overwrites_previous() {
        let mut store = StateStore {
            path: String::new(),
            signals: HashMap::new(),
        };

        let buy = make_signal(SignalType::Buy);
        store.update(&buy);
        assert_eq!(store.signals["TEST"].signal_type, SignalType::Buy);

        let sell = make_signal(SignalType::Sell);
        store.update(&sell);
        assert_eq!(store.signals["TEST"].signal_type, SignalType::Sell);
    }
}
