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
pub struct BinanceProvider {
    agent: ureq::Agent,
}

impl BinanceProvider {
    pub fn new() -> Self {
        Self {
            agent: crate::http::agent(),
        }
    }

    /// Converts watchlist symbol format to a Binance trading pair.
    ///
    /// Replaces `-USD` with `USDT` and strips remaining hyphens.
    /// Example: `BTC-USD` → `BTCUSDT`, `SOL-USD` → `SOLUSDT`.
    fn map_symbol(symbol: &str) -> String {
        symbol
            .to_uppercase()
            .replace("-USD", "USDT")
            .replace('-', "")
    }
}

impl DataProvider for BinanceProvider {
    fn fetch_candles(&self, symbol: &str) -> anyhow::Result<Vec<Candle>> {
        let mapped = Self::map_symbol(symbol);
        let url =
            format!("https://api.binance.com/api/v3/klines?symbol={mapped}&interval=1d&limit=100");

        log::info!("{symbol}: fetching from binance (mapped: {mapped})");
        log::trace!("{symbol}: GET {url}");

        let response: Vec<Vec<serde_json::Value>> = self
            .agent
            .get(&url)
            .call()
            .with_context(|| format!("{symbol}: binance HTTP request failed"))?
            .body_mut()
            .read_json()
            .with_context(|| format!("{symbol}: failed to parse binance JSON response"))?;

        if response.is_empty() {
            bail!("{symbol}: binance returned empty response (mapped: {mapped})");
        }

        let parsed: Vec<Candle> = response.iter().filter_map(|k| parse_kline(k)).collect();

        if parsed.is_empty() {
            bail!("{symbol}: all klines failed to parse (mapped: {mapped})");
        }

        let total = parsed.len();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let candles = drop_unclosed(parsed, now);
        if candles.len() < total {
            log::debug!(
                "{symbol}: dropped {} unclosed candle(s) for the current UTC day",
                total - candles.len()
            );
        }
        if candles.is_empty() {
            bail!("{symbol}: no closed candles remain after dropping the current UTC day");
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

/// Removes candles belonging to the current (still open) UTC day.
///
/// Binance daily klines open at 00:00 UTC and the API includes the in-progress
/// candle; a crossover computed on it can reverse before close. CDC Action Zone
/// signals on bar close, so only fully closed candles are analyzed.
fn drop_unclosed(mut candles: Vec<Candle>, now_utc: i64) -> Vec<Candle> {
    let day_start = now_utc - now_utc.rem_euclid(86_400);
    candles.retain(|c| c.timestamp < day_start);
    candles
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
    fn test_symbol_mapping_lowercase_input() {
        // Lowercase input must map to the same pair as uppercase
        assert_eq!(BinanceProvider::map_symbol("btc-usd"), "BTCUSDT");
        assert_eq!(BinanceProvider::map_symbol("BTC-USD"), "BTCUSDT");
    }

    #[test]
    fn test_symbol_mapping_no_hyphen() {
        assert_eq!(BinanceProvider::map_symbol("BTCUSDT"), "BTCUSDT");
    }

    const DAY: i64 = 86_400;
    /// 2024-01-01T00:00:00Z — a UTC day boundary.
    const DAY_START: i64 = 1_704_067_200;

    fn make_candle(timestamp: i64) -> Candle {
        Candle {
            timestamp,
            open: 1.0,
            high: 2.0,
            low: 0.5,
            close: 1.5,
            volume: 100.0,
        }
    }

    #[test]
    fn test_drop_unclosed_removes_current_day_candle() {
        let candles = vec![
            make_candle(DAY_START - 2 * DAY),
            make_candle(DAY_START - DAY),
            make_candle(DAY_START),
        ];
        // now = 22:00 UTC on the day that opened at DAY_START
        let result = drop_unclosed(candles, DAY_START + 79_200);
        assert_eq!(result.len(), 2);
        assert_eq!(result.last().unwrap().timestamp, DAY_START - DAY);
    }

    #[test]
    fn test_drop_unclosed_keeps_all_closed_candles() {
        let candles = vec![
            make_candle(DAY_START - 2 * DAY),
            make_candle(DAY_START - DAY),
        ];
        let result = drop_unclosed(candles, DAY_START + 60);
        assert_eq!(result.len(), 2);
    }
}
