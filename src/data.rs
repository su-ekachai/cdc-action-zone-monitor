//! Market data fetching layer.
//!
//! Provides a [`DataProvider`] trait and concrete implementations for
//! Yahoo Finance and Binance. Each provider returns chronologically-ordered
//! daily OHLCV candles.

pub mod binance;
pub mod yahoo;

/// A single daily OHLCV (Open-High-Low-Close-Volume) candle.
///
/// Timestamps are Unix epoch seconds in UTC. Price fields use the quote currency
/// of the trading pair (USD for stocks, USDT for Binance pairs).
// ponytail: open/high/low are parsed to validate each API row (a null/garbage
// field rejects the candle) but unread — the strategy is close+volume only.
// Restore reads if you add range-based indicators (ATR, candlestick patterns).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Candle {
    /// Unix timestamp in seconds (start of daily candle, UTC).
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// Trading volume in base asset units.
    pub volume: f64,
}

/// Trait for market data providers returning daily OHLCV candles.
///
/// Implementations handle symbol format translation, HTTP requests, and
/// response parsing specific to each exchange or data vendor.
pub trait DataProvider {
    /// Fetches approximately 3 months of daily candles for the given symbol.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, invalid symbol, rate limiting,
    /// or empty response data.
    fn fetch_candles(&self, symbol: &str) -> anyhow::Result<Vec<Candle>>;
}

/// Formats a Unix timestamp as an ISO 8601 date string (`YYYY-MM-DD`).
///
/// Returns `"?"` for out-of-range timestamps.
pub fn format_date(timestamp: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(timestamp)
        .map_or_else(|_| "?".to_string(), |dt| dt.date().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_date_known_timestamp() {
        assert_eq!(format_date(1_704_067_200), "2024-01-01");
    }

    #[test]
    fn test_format_date_epoch_zero() {
        assert_eq!(format_date(0), "1970-01-01");
    }

    #[test]
    fn test_format_date_out_of_range() {
        assert_eq!(format_date(i64::MAX), "?");
    }

    #[test]
    fn test_format_date_negative_valid() {
        assert_eq!(format_date(-86400), "1969-12-31");
    }
}
