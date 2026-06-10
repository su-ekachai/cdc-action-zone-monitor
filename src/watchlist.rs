//! Watchlist management backed by the `[[watchlist]]` array in `config.toml`.
//!
//! Provides load, add, remove, and list operations. Mutations use `toml_edit`
//! to preserve existing comments and formatting in the config file.

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

/// Market data provider for a watchlist symbol.
///
/// Selection is either explicit (via `--source` flag) or auto-detected by
/// [`detect_source`] based on the symbol format:
/// - Symbols matching `*-USD` without a period route to Binance (e.g., `BTC-USD`).
/// - All other symbols route to Yahoo Finance (e.g., `AAPL`, `PTT.BK`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DataSource {
    Yahoo,
    Binance,
}

impl std::fmt::Display for DataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Yahoo => write!(f, "yahoo"),
            Self::Binance => write!(f, "binance"),
        }
    }
}

impl std::str::FromStr for DataSource {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.to_lowercase().as_str() {
            "yahoo" => Ok(Self::Yahoo),
            "binance" => Ok(Self::Binance),
            other => bail!("Unknown data source: {other}. Use 'yahoo' or 'binance'."),
        }
    }
}

/// A single entry in the `[[watchlist]]` TOML array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistEntry {
    pub symbol: String,
    pub source: DataSource,
}

impl WatchlistEntry {
    pub fn with_auto_source(symbol: &str, source: Option<DataSource>) -> Self {
        // Normalize to uppercase so hand-edited lowercase entries still route
        // and map correctly (Binance pair mapping is built from this form).
        let symbol = symbol.to_uppercase();
        let source = source.unwrap_or_else(|| detect_source(&symbol));
        Self { symbol, source }
    }
}

/// Intermediate struct allowing optional `source` during TOML deserialization.
#[derive(Debug, Deserialize)]
struct RawWatchlistEntry {
    symbol: String,
    source: Option<DataSource>,
}

/// Parses `[[watchlist]]` entries from a TOML config file.
///
/// When the `source` field is omitted for an entry, the provider is
/// auto-detected from the symbol format via [`detect_source`].
///
/// # Errors
///
/// Returns an error if the file cannot be read or contains invalid TOML.
pub fn load(path: &str) -> anyhow::Result<Vec<WatchlistEntry>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {path}"))?;

    #[derive(Deserialize)]
    struct Partial {
        watchlist: Option<Vec<RawWatchlistEntry>>,
    }

    let parsed: Partial =
        toml::from_str(&content).with_context(|| format!("Failed to parse config file: {path}"))?;

    let entries = parsed
        .watchlist
        .unwrap_or_default()
        .into_iter()
        .map(|raw| WatchlistEntry::with_auto_source(&raw.symbol, raw.source))
        .collect();

    Ok(entries)
}

/// Appends a symbol to the `[[watchlist]]` array in the config file.
///
/// The symbol is uppercased before insertion. If no source is provided,
/// the data provider is auto-detected from the symbol format.
///
/// # Errors
///
/// Returns an error if the symbol already exists or the file is unwritable.
pub fn add(path: &str, symbol: &str, source: Option<DataSource>) -> anyhow::Result<()> {
    let existing = load(path)?;
    let upper = symbol.to_uppercase();

    if existing.iter().any(|e| e.symbol == upper) {
        bail!("{upper} is already in the watchlist");
    }

    let source = source.unwrap_or_else(|| detect_source(&upper));

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {path}"))?;
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("Failed to parse config file for editing: {path}"))?;

    let mut table = toml_edit::Table::new();
    table.set_implicit(true);
    table.insert("symbol", toml_edit::value(&upper));
    table.insert("source", toml_edit::value(source.to_string()));

    if let Some(array) = doc.get_mut("watchlist") {
        if let Some(arr) = array.as_array_of_tables_mut() {
            arr.push(table);
        } else {
            bail!("'watchlist' in config is not an array of tables");
        }
    } else {
        let mut arr = toml_edit::ArrayOfTables::new();
        arr.push(table);
        doc.insert("watchlist", toml_edit::Item::ArrayOfTables(arr));
    }

    crate::fsutil::write_atomic(path, &doc.to_string())
        .with_context(|| format!("Failed to write config file: {path}"))?;

    log::debug!("Appended [[watchlist]] entry: symbol={upper}, source={source}");

    use owo_colors::OwoColorize;
    use owo_colors::Stream::Stdout;
    println!(
        "{} Added {upper} (source: {source})",
        "\u{2713}".if_supports_color(Stdout, |s| s.green())
    );
    Ok(())
}

/// Removes a symbol from the `[[watchlist]]` array (case-insensitive match).
///
/// # Errors
///
/// Returns an error if the config file cannot be read or written.
pub fn remove(path: &str, symbol: &str) -> anyhow::Result<()> {
    let upper = symbol.to_uppercase();

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {path}"))?;
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("Failed to parse config file for editing: {path}"))?;

    if let Some(arr) = doc
        .get_mut("watchlist")
        .and_then(|a| a.as_array_of_tables_mut())
    {
        let mut i = 0;
        let mut found = false;
        while i < arr.len() {
            let matches = arr
                .get(i)
                .and_then(|t| t.get("symbol"))
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.to_uppercase() == upper);
            if matches {
                arr.remove(i);
                found = true;
            } else {
                i += 1;
            }
        }
        if !found {
            use owo_colors::OwoColorize;
            use owo_colors::Stream::Stdout;
            println!(
                "{} {upper} not found in watchlist",
                "\u{26a0}".if_supports_color(Stdout, |s| s.yellow())
            );
            return Ok(());
        }
    }

    crate::fsutil::write_atomic(path, &doc.to_string())
        .with_context(|| format!("Failed to write config file: {path}"))?;

    log::debug!("Removed [[watchlist]] entry: symbol={upper}");

    use owo_colors::OwoColorize;
    use owo_colors::Stream::Stdout;
    println!(
        "{} Removed {upper}",
        "\u{2713}".if_supports_color(Stdout, |s| s.green())
    );
    Ok(())
}

/// Outputs all watchlist entries as a formatted table or JSON.
///
/// # Errors
///
/// Returns an error if the config file cannot be loaded.
pub fn list(path: &str, json: bool) -> anyhow::Result<()> {
    let entries = load(path)?;
    if entries.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("Watchlist is empty. Use 'add <SYMBOL>' to get started.");
        }
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        use owo_colors::OwoColorize;
        use owo_colors::Stream::Stdout;

        println!("{:<15} {:<10}", "SYMBOL", "SOURCE");
        println!("{}", "-".repeat(25));
        for entry in &entries {
            println!(
                "{:<15} {:<10}",
                entry.symbol.if_supports_color(Stdout, |s| s.bold()),
                entry.source
            );
        }
        println!("\n{} symbols total.", entries.len());
    }
    Ok(())
}

/// Infers the data provider from a symbol's naming convention.
///
/// Symbols matching `*-USD` without a period (e.g., `BTC-USD`, `SOL-USD`)
/// route to Binance. All others (e.g., `AAPL`, `PTT.BK`) route to Yahoo Finance.
pub fn detect_source(symbol: &str) -> DataSource {
    if symbol.contains("-USD") && !symbol.contains('.') {
        DataSource::Binance
    } else {
        DataSource::Yahoo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_test_config(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn test_detect_source() {
        assert_eq!(detect_source("BTC-USD"), DataSource::Binance);
        assert_eq!(detect_source("ETH-USD"), DataSource::Binance);
        assert_eq!(detect_source("AAPL"), DataSource::Yahoo);
        assert_eq!(detect_source("PTT.BK"), DataSource::Yahoo);
    }

    #[test]
    fn test_data_source_parse() {
        assert_eq!("yahoo".parse::<DataSource>().unwrap(), DataSource::Yahoo);
        assert_eq!(
            "binance".parse::<DataSource>().unwrap(),
            DataSource::Binance
        );
        assert!("invalid".parse::<DataSource>().is_err());
    }

    #[test]
    fn test_load_with_source() {
        let f = write_test_config(
            r#"
[[watchlist]]
symbol = "BTC-USD"
source = "binance"

[[watchlist]]
symbol = "AAPL"
source = "yahoo"
"#,
        );
        let entries = load(f.path().to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].symbol, "BTC-USD");
        assert_eq!(entries[0].source, DataSource::Binance);
        assert_eq!(entries[1].symbol, "AAPL");
        assert_eq!(entries[1].source, DataSource::Yahoo);
    }

    #[test]
    fn test_load_normalizes_symbol_case() {
        // Hand-edited lowercase entries must still route to Binance and
        // display in canonical uppercase form.
        let f = write_test_config(
            r#"
[[watchlist]]
symbol = "btc-usd"
"#,
        );
        let entries = load(f.path().to_str().unwrap()).unwrap();
        assert_eq!(entries[0].symbol, "BTC-USD");
        assert_eq!(entries[0].source, DataSource::Binance);
    }

    #[test]
    fn test_load_auto_detect_source() {
        let f = write_test_config(
            r#"
[[watchlist]]
symbol = "ETH-USD"

[[watchlist]]
symbol = "PTT.BK"
"#,
        );
        let entries = load(f.path().to_str().unwrap()).unwrap();
        assert_eq!(entries[0].source, DataSource::Binance);
        assert_eq!(entries[1].source, DataSource::Yahoo);
    }

    #[test]
    fn test_add_and_remove() {
        let f = write_test_config(
            r#"
[settings]
state_file = "test.json"

[[watchlist]]
symbol = "BTC-USD"
source = "binance"
"#,
        );
        let path = f.path().to_str().unwrap();

        add(path, "sol-usd", None).unwrap();
        let entries = load(path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].symbol, "SOL-USD");
        assert_eq!(entries[1].source, DataSource::Binance);

        remove(path, "SOL-USD").unwrap();
        let entries = load(path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].symbol, "BTC-USD");
    }

    #[test]
    fn test_add_duplicate_fails() {
        let f = write_test_config(
            r#"
[[watchlist]]
symbol = "BTC-USD"
source = "binance"
"#,
        );
        let path = f.path().to_str().unwrap();
        let result = add(path, "btc-usd", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_watchlist() {
        let f = write_test_config(
            r#"
[settings]
state_file = "test.json"
"#,
        );
        let entries = load(f.path().to_str().unwrap()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_list_empty_does_not_error() {
        let f = write_test_config("[settings]\nstate_file = \"t.json\"\n");
        let result = list(f.path().to_str().unwrap(), false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_json_does_not_error() {
        let f = write_test_config(
            r#"
[[watchlist]]
symbol = "BTC-USD"
source = "binance"
"#,
        );
        let result = list(f.path().to_str().unwrap(), true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_plain_does_not_error() {
        let f = write_test_config(
            r#"
[[watchlist]]
symbol = "BTC-USD"
source = "binance"
"#,
        );
        let result = list(f.path().to_str().unwrap(), false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_nonexistent_symbol_does_not_error() {
        let f = write_test_config(
            r#"
[[watchlist]]
symbol = "BTC-USD"
source = "binance"
"#,
        );
        let result = remove(f.path().to_str().unwrap(), "NONEXIST");
        assert!(result.is_ok());
    }

    #[test]
    fn test_detect_source_usd_with_dot_is_yahoo() {
        assert_eq!(detect_source("USD.BK"), DataSource::Yahoo);
    }

    #[test]
    fn test_data_source_display() {
        assert_eq!(format!("{}", DataSource::Yahoo), "yahoo");
        assert_eq!(format!("{}", DataSource::Binance), "binance");
    }

    #[test]
    fn test_data_source_parse_case_insensitive() {
        assert_eq!("YAHOO".parse::<DataSource>().unwrap(), DataSource::Yahoo);
        assert_eq!(
            "Binance".parse::<DataSource>().unwrap(),
            DataSource::Binance
        );
    }

    #[test]
    fn test_with_auto_source_explicit_override() {
        let entry = WatchlistEntry::with_auto_source("BTC-USD", Some(DataSource::Yahoo));
        assert_eq!(entry.source, DataSource::Yahoo);
    }

    #[test]
    fn test_load_missing_file_returns_error() {
        let result = load("/nonexistent/config.toml");
        assert!(result.is_err());
    }
}
