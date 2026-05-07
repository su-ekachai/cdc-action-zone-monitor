//! Yahoo Finance v8 chart API data provider.
//!
//! Fetches 3 months of daily OHLCV data via the public unauthenticated endpoint.
//! Null candle fields (common on market holidays) are silently skipped.

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::data::{Candle, DataProvider, format_date};

/// Fetches daily candles from the Yahoo Finance v8 chart API.
///
/// Suitable for equities, ETFs, and indices (e.g., `AAPL`, `PTT.BK`, `^GSPC`).
pub struct YahooProvider;

impl YahooProvider {
    pub fn new() -> Self {
        Self
    }
}

impl DataProvider for YahooProvider {
    fn fetch_candles(&self, symbol: &str) -> anyhow::Result<Vec<Candle>> {
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?interval=1d&range=3mo"
        );

        log::info!("{symbol}: fetching from yahoo finance");
        log::trace!("{symbol}: GET {url}");

        let response: YahooResponse = ureq::get(&url)
            .header("User-Agent", "Mozilla/5.0 (compatible; CDCAZMonitor/0.1)")
            .call()
            .with_context(|| format!("{symbol}: yahoo HTTP request failed"))?
            .body_mut()
            .read_json()
            .with_context(|| format!("{symbol}: failed to parse yahoo JSON response"))?;

        parse_response(response, symbol)
    }
}

fn parse_response(response: YahooResponse, symbol: &str) -> anyhow::Result<Vec<Candle>> {
    let result = response
        .chart
        .result
        .into_iter()
        .next()
        .with_context(|| format!("{symbol}: no data in yahoo response"))?;

    let timestamps = result.timestamp.unwrap_or_default();
    let quote = result
        .indicators
        .quote
        .into_iter()
        .next()
        .with_context(|| format!("{symbol}: no quote data in yahoo response"))?;

    if timestamps.is_empty() {
        bail!("{symbol}: yahoo returned no timestamps");
    }

    let candles: Vec<Candle> = timestamps
        .iter()
        .enumerate()
        .filter_map(|(i, &ts)| {
            Some(Candle {
                timestamp: ts,
                open: *quote.open.get(i)?.as_ref()?,
                high: *quote.high.get(i)?.as_ref()?,
                low: *quote.low.get(i)?.as_ref()?,
                close: *quote.close.get(i)?.as_ref()?,
                volume: *quote.volume.get(i)?.as_ref()?,
            })
        })
        .collect();

    if candles.is_empty() {
        bail!("{symbol}: all candle rows had null fields, 0 valid candles after filtering");
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

#[derive(Deserialize)]
struct YahooResponse {
    chart: YahooChart,
}

#[derive(Deserialize)]
struct YahooChart {
    result: Vec<YahooResult>,
}

#[derive(Deserialize)]
struct YahooResult {
    timestamp: Option<Vec<i64>>,
    indicators: YahooIndicators,
}

#[derive(Deserialize)]
struct YahooIndicators {
    quote: Vec<YahooQuote>,
}

#[derive(Deserialize)]
struct YahooQuote {
    open: Vec<Option<f64>>,
    high: Vec<Option<f64>>,
    low: Vec<Option<f64>>,
    close: Vec<Option<f64>>,
    volume: Vec<Option<f64>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_response(
        timestamps: Vec<i64>,
        open: Vec<Option<f64>>,
        high: Vec<Option<f64>>,
        low: Vec<Option<f64>>,
        close: Vec<Option<f64>>,
        volume: Vec<Option<f64>>,
    ) -> YahooResponse {
        YahooResponse {
            chart: YahooChart {
                result: vec![YahooResult {
                    timestamp: Some(timestamps),
                    indicators: YahooIndicators {
                        quote: vec![YahooQuote {
                            open,
                            high,
                            low,
                            close,
                            volume,
                        }],
                    },
                }],
            },
        }
    }

    #[test]
    fn test_parse_response_valid() {
        let response = make_response(
            vec![1_704_067_200, 1_704_153_600],
            vec![Some(150.0), Some(152.0)],
            vec![Some(155.0), Some(157.0)],
            vec![Some(148.0), Some(150.0)],
            vec![Some(153.0), Some(156.0)],
            vec![Some(1_000_000.0), Some(1_200_000.0)],
        );

        let candles = parse_response(response, "AAPL").unwrap();
        assert_eq!(candles.len(), 2);
        assert_eq!(candles[0].timestamp, 1_704_067_200);
        assert!((candles[0].open - 150.0).abs() < 1e-10);
        assert!((candles[0].close - 153.0).abs() < 1e-10);
        assert!((candles[1].volume - 1_200_000.0).abs() < 1e-10);
    }

    #[test]
    fn test_parse_response_null_fields_skipped() {
        let response = make_response(
            vec![1_704_067_200, 1_704_153_600, 1_704_240_000],
            vec![Some(150.0), None, Some(152.0)],
            vec![Some(155.0), None, Some(157.0)],
            vec![Some(148.0), None, Some(150.0)],
            vec![Some(153.0), None, Some(156.0)],
            vec![Some(1_000_000.0), None, Some(1_200_000.0)],
        );

        let candles = parse_response(response, "AAPL").unwrap();
        assert_eq!(candles.len(), 2);
        assert_eq!(candles[0].timestamp, 1_704_067_200);
        assert_eq!(candles[1].timestamp, 1_704_240_000);
    }

    #[test]
    fn test_parse_response_empty_timestamps_returns_error() {
        let response = YahooResponse {
            chart: YahooChart {
                result: vec![YahooResult {
                    timestamp: Some(vec![]),
                    indicators: YahooIndicators {
                        quote: vec![YahooQuote {
                            open: vec![],
                            high: vec![],
                            low: vec![],
                            close: vec![],
                            volume: vec![],
                        }],
                    },
                }],
            },
        };

        let result = parse_response(response, "AAPL");
        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("no timestamps"));
    }

    #[test]
    fn test_parse_response_no_results_returns_error() {
        let response = YahooResponse {
            chart: YahooChart { result: vec![] },
        };

        let result = parse_response(response, "AAPL");
        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("no data"));
    }

    #[test]
    fn test_parse_response_all_nulls_returns_error() {
        let response = make_response(
            vec![1_704_067_200],
            vec![None],
            vec![None],
            vec![None],
            vec![None],
            vec![None],
        );

        let result = parse_response(response, "AAPL");
        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("null fields"));
    }
}
