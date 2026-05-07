//! Binance public klines API data provider.
//!
//! Fetches 100 daily OHLCV candles from the unauthenticated `/api/v3/klines` endpoint.
//! No API key is required. Symbols are mapped from watchlist format (`BTC-USD`)
//! to Binance format (`BTCUSDT`) via [`BinanceProvider::map_symbol`].

use anyhow::{Context, bail};

use crate::data::{Candle, DataProvider, format_date};

/// Fetches daily candles from the Binance spot market klines API.
///
/// Suitable for cryptocurrency pairs traded against USDT on Binance.
pub struct BinanceProvider;

impl BinanceProvider {
    pub fn new() -> Self {
        Self
    }

    /// Converts watchlist symbol format to a Binance trading pair.
    ///
    /// Replaces `-USD` with `USDT` and strips remaining hyphens.
    /// Example: `BTC-USD` → `BTCUSDT`, `SOL-USD` → `SOLUSDT`.
    fn map_symbol(symbol: &str) -> String {
        symbol
            .replace("-USD", "USDT")
            .replace('-', "")
            .to_uppercase()
    }
}

impl DataProvider for BinanceProvider {
    fn fetch_candles(&self, symbol: &str) -> anyhow::Result<Vec<Candle>> {
        let mapped = Self::map_symbol(symbol);
        let url =
            format!("https://api.binance.com/api/v3/klines?symbol={mapped}&interval=1d&limit=100");

        log::info!("{symbol}: fetching from binance (mapped: {mapped})");
        log::trace!("{symbol}: GET {url}");

        let response: Vec<Vec<serde_json::Value>> = ureq::get(&url)
            .call()
            .with_context(|| format!("{symbol}: binance HTTP request failed"))?
            .body_mut()
            .read_json()
            .with_context(|| format!("{symbol}: failed to parse binance JSON response"))?;

        if response.is_empty() {
            bail!("{symbol}: binance returned empty response (mapped: {mapped})");
        }

        let candles: Vec<Candle> = response.iter().filter_map(|k| parse_kline(k)).collect();

        if candles.is_empty() {
            bail!("{symbol}: all klines failed to parse (mapped: {mapped})");
        }

        let first_ts = candles.first().map_or(0, |c| c.timestamp);
        let last_ts = candles.last().map_or(0, |c| c.timestamp);
        log::info!(
            "{symbol}: {} candles received (range: {}..{})",
            candles.len(),
            format_date(first_ts),
            format_date(last_ts)
        );
        Ok(candles)
    }
}

/// Parses a single Binance kline JSON array into a [`Candle`].
///
/// Binance klines encode OHLCV values as strings within a JSON array.
/// Timestamps arrive in milliseconds and are converted to seconds.
/// Returns `None` if the array has fewer than 6 elements or any field
/// fails to parse.
fn parse_kline(k: &[serde_json::Value]) -> Option<Candle> {
    if k.len() < 6 {
        return None;
    }
    Some(Candle {
        timestamp: k[0].as_i64()? / 1000,
        open: k[1].as_str()?.parse().ok()?,
        high: k[2].as_str()?.parse().ok()?,
        low: k[3].as_str()?.parse().ok()?,
        close: k[4].as_str()?.parse().ok()?,
        volume: k[5].as_str()?.parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_mapping() {
        assert_eq!(BinanceProvider::map_symbol("BTC-USD"), "BTCUSDT");
        assert_eq!(BinanceProvider::map_symbol("ETH-USD"), "ETHUSDT");
        assert_eq!(BinanceProvider::map_symbol("SOL-USD"), "SOLUSDT");
    }

    #[test]
    fn test_parse_kline_valid() {
        let k: Vec<serde_json::Value> = vec![
            serde_json::json!(1_704_067_200_000_i64),
            serde_json::json!("42000.00"),
            serde_json::json!("43000.00"),
            serde_json::json!("41000.00"),
            serde_json::json!("42500.00"),
            serde_json::json!("1000.5"),
        ];
        let candle = parse_kline(&k).unwrap();
        assert_eq!(candle.timestamp, 1_704_067_200);
        assert!((candle.open - 42000.0).abs() < 1e-10);
        assert!((candle.close - 42500.0).abs() < 1e-10);
    }

    #[test]
    fn test_parse_kline_insufficient_fields() {
        let k: Vec<serde_json::Value> = vec![serde_json::json!(123)];
        assert!(parse_kline(&k).is_none());
    }

    #[test]
    fn test_parse_kline_empty_array() {
        let k: Vec<serde_json::Value> = vec![];
        assert!(parse_kline(&k).is_none());
    }

    #[test]
    fn test_parse_kline_null_timestamp() {
        let k: Vec<serde_json::Value> = vec![
            serde_json::json!(null),
            serde_json::json!("42000.00"),
            serde_json::json!("43000.00"),
            serde_json::json!("41000.00"),
            serde_json::json!("42500.00"),
            serde_json::json!("1000.5"),
        ];
        assert!(parse_kline(&k).is_none());
    }

    #[test]
    fn test_parse_kline_non_numeric_string() {
        let k: Vec<serde_json::Value> = vec![
            serde_json::json!(1_704_067_200_000_i64),
            serde_json::json!("not_a_number"),
            serde_json::json!("43000.00"),
            serde_json::json!("41000.00"),
            serde_json::json!("42500.00"),
            serde_json::json!("1000.5"),
        ];
        assert!(parse_kline(&k).is_none());
    }

    #[test]
    fn test_symbol_mapping_preserves_usd_case_sensitivity() {
        // map_symbol expects the canonical format "*-USD"; lowercase won't match the replace
        assert_eq!(BinanceProvider::map_symbol("btc-usd"), "BTCUSD");
        // Uppercase works correctly
        assert_eq!(BinanceProvider::map_symbol("BTC-USD"), "BTCUSDT");
    }

    #[test]
    fn test_symbol_mapping_no_hyphen() {
        assert_eq!(BinanceProvider::map_symbol("BTCUSDT"), "BTCUSDT");
    }
}
