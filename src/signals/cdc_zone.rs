//! CDC Action Zone crossover detection.
//!
//! Implements the original CDC Action Zone strategy (`piriya33`, `TradingView`).
//! A signal fires when EMA(12) crosses EMA(26) on the most recent candle.
//! Zone classification and strength indicators provide context but do not
//! gate signal emission.

use crate::data::Candle;
use crate::signals::indicators::{ema, rsi, sma, volume_ratio};
use crate::signals::{Signal, SignalType, Zone};

const FAST_MA_PERIOD: usize = 12;
const SLOW_MA_PERIOD: usize = 26;
const TREND_MA_PERIOD: usize = 50;
const RSI_PERIOD: usize = 14;
const VOLUME_MA_PERIOD: usize = 20;

/// Minimum candles required for valid analysis: `TREND_MA_PERIOD` (50) + 2
/// (current and previous EMA values needed for crossover detection).
const MIN_CANDLES: usize = 52;

/// Analyzes a candle series for an EMA crossover on the most recent bar.
///
/// Computes EMA(12), EMA(26), RSI(14), volume ratio, and SMA(50), then checks
/// whether the fast EMA crossed above (buy) or below (sell) the slow EMA between
/// the last two candles.
///
/// Returns `Some(Signal)` on crossover detection, `None` otherwise.
/// Also returns `None` if fewer than [`MIN_CANDLES`] are provided or EMA values
/// contain `NAN` due to insufficient warm-up.
pub fn analyze(symbol: &str, candles: &[Candle]) -> Option<Signal> {
    if candles.len() < MIN_CANDLES {
        log::warn!(
            "{symbol}: insufficient data ({} candles, need {MIN_CANDLES})",
            candles.len()
        );
        return None;
    }

    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let volumes: Vec<f64> = candles.iter().map(|c| c.volume).collect();

    let fast = ema(&closes, FAST_MA_PERIOD);
    let slow = ema(&closes, SLOW_MA_PERIOD);
    let trend = sma(&closes, TREND_MA_PERIOD);
    let rsi_vals = rsi(&closes, RSI_PERIOD);
    let vol_ratio = volume_ratio(&volumes, VOLUME_MA_PERIOD);

    let last = candles.len() - 1;
    let prev = last - 1;

    let fast_now = fast[last];
    let fast_prev = fast[prev];
    let slow_now = slow[last];
    let slow_prev = slow[prev];

    if fast_now.is_nan() || slow_now.is_nan() || fast_prev.is_nan() || slow_prev.is_nan() {
        log::debug!("{symbol}: EMA values contain NaN — insufficient warm-up data");
        return None;
    }

    let close = closes[last];
    let current_rsi = if rsi_vals[last].is_nan() {
        0.0
    } else {
        rsi_vals[last]
    };
    let current_vol_ratio = if vol_ratio[last].is_nan() {
        0.0
    } else {
        vol_ratio[last]
    };
    let current_trend = if trend[last].is_nan() {
        close
    } else {
        trend[last]
    };

    log::debug!(
        "{symbol}: EMA12={fast_now:.2} EMA26={slow_now:.2} RSI={current_rsi:.1} vol={current_vol_ratio:.2}x SMA50={current_trend:.2}",
    );

    let crossover = fast_prev <= slow_prev && fast_now > slow_now;
    let crossunder = fast_prev >= slow_prev && fast_now < slow_now;

    if !crossover && !crossunder {
        log::debug!(
            "{symbol}: no crossover (EMA12 prev={fast_prev:.2}, EMA26 prev={slow_prev:.2})"
        );
        return None;
    }

    log::info!(
        "{symbol}: {} detected — EMA12 {direction} EMA26",
        if crossover {
            "BUY crossover"
        } else {
            "SELL crossunder"
        },
        direction = if crossover {
            "crossed above"
        } else {
            "crossed below"
        },
    );

    let zone = determine_zone(fast_now, slow_now, close);

    let signal_type = if crossover {
        SignalType::Buy
    } else {
        SignalType::Sell
    };

    Some(Signal {
        symbol: symbol.to_string(),
        signal_type,
        zone,
        price: close,
        fast_ema: fast_now,
        slow_ema: slow_now,
        rsi: current_rsi,
        volume_ratio: current_vol_ratio,
        trend_sma50: current_trend,
        timestamp: candles[last].timestamp,
    })
}

/// Classifies the market zone based on the relationship between price, fast EMA, and slow EMA.
fn determine_zone(fast: f64, slow: f64, close: f64) -> Zone {
    if fast > slow {
        if close > fast {
            Zone::Bull
        } else {
            Zone::WeakBull
        }
    } else if close < fast {
        Zone::Bear
    } else {
        Zone::WeakBear
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_candles(closes: &[f64]) -> Vec<Candle> {
        closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Candle {
                #[allow(clippy::cast_possible_wrap)]
                timestamp: i as i64,
                open: c,
                high: c + 1.0,
                low: c - 1.0,
                close: c,
                volume: 1000.0,
            })
            .collect()
    }

    #[test]
    fn test_no_signal_on_insufficient_data() {
        let candles = make_candles(&vec![100.0; 30]);
        assert!(analyze("TEST", &candles).is_none());
    }

    #[test]
    fn test_no_signal_on_flat_data() {
        let candles = make_candles(&vec![100.0; 60]);
        assert!(analyze("TEST", &candles).is_none());
    }

    #[test]
    fn test_buy_signal_on_uptrend() {
        // 52 declining bars (200 down to 149) push EMA12 below EMA26 since the
        // faster EMA tracks the decline more closely. A single large spike on
        // the final bar (250) lifts EMA12 above EMA26, triggering a crossover.
        //
        // Verified numerically:
        //   bar[51]: EMA12=154.50 <= EMA26=161.50  (no cross yet)
        //   bar[52]: EMA12=169.19 >  EMA26=168.06  (cross!)
        let mut closes: Vec<f64> = (0..52).map(|i| 200.0 - f64::from(i)).collect();
        closes.push(250.0);

        let candles = make_candles(&closes);
        let signal = analyze("TEST", &candles).expect("expected buy signal on crossover");
        assert_eq!(signal.signal_type, SignalType::Buy);
    }

    #[test]
    fn test_sell_signal_on_downtrend() {
        // 52 rising bars (100 up to 151) push EMA12 above EMA26 since the faster
        // EMA tracks the incline more closely. A single large drop on the final
        // bar (50) pulls EMA12 below EMA26, triggering a crossunder.
        //
        // Verified numerically:
        //   bar[51]: EMA12=145.50 >= EMA26=138.50  (no cross yet)
        //   bar[52]: EMA12=130.81 <  EMA26=131.94  (cross!)
        let mut closes: Vec<f64> = (0..52).map(|i| 100.0 + f64::from(i)).collect();
        closes.push(50.0);

        let candles = make_candles(&closes);
        let signal = analyze("TEST", &candles).expect("expected sell signal on crossunder");
        assert_eq!(signal.signal_type, SignalType::Sell);
    }

    #[test]
    fn test_zone_determination() {
        assert_eq!(determine_zone(10.0, 8.0, 12.0), Zone::Bull);
        assert_eq!(determine_zone(10.0, 8.0, 9.0), Zone::WeakBull);
        assert_eq!(determine_zone(8.0, 10.0, 7.0), Zone::Bear);
        assert_eq!(determine_zone(8.0, 10.0, 9.0), Zone::WeakBear);
    }

    /// Verifies that a buy crossover fires when EMAs are exactly equal on the
    /// previous bar and EMA12 rises above EMA26 on the current bar.
    ///
    /// The crossover condition is: `fast_prev <= slow_prev && fast_now > slow_now`
    /// The `<=` (not `<`) means equality on the previous bar counts as a valid
    /// starting point for a bullish cross.
    ///
    /// Setup: 52 bars at constant 100.0 (making EMA12 == EMA26 == 100.0),
    /// then one spike to 102.0. Since EMA12 reacts faster (k=2/13=0.1538)
    /// than EMA26 (k=2/27=0.0741), the spike lifts EMA12 above EMA26.
    ///
    /// Expected:
    ///   prev: EMA12 = 100.0, EMA26 = 100.0 (equal => satisfies <=)
    ///   now:  EMA12 = (102-100)*0.1538 + 100 = 100.3077
    ///         EMA26 = (102-100)*0.0741 + 100 = 100.1481
    ///   EMA12 > EMA26 => crossover detected!
    #[test]
    fn test_crossover_from_equality() {
        // 52 flat bars + 1 spike = 53 candles total (>= MIN_CANDLES of 52)
        let mut closes = vec![100.0; 52];
        closes.push(102.0);

        let candles = make_candles(&closes);
        let signal =
            analyze("TEST-EQ", &candles).expect("expected buy signal when crossing from equality");

        assert_eq!(signal.signal_type, SignalType::Buy);

        // Verify the EMA values are what we expect
        let k12 = 2.0 / 13.0; // 0.153846...
        let k26 = 2.0 / 27.0; // 0.074074...
        let expected_fast = (102.0 - 100.0) * k12 + 100.0; // 100.3077
        let expected_slow = (102.0 - 100.0) * k26 + 100.0; // 100.1481

        assert!(
            (signal.fast_ema - expected_fast).abs() < 0.01,
            "fast_ema: got {:.4}, expected {:.4}",
            signal.fast_ema,
            expected_fast
        );
        assert!(
            (signal.slow_ema - expected_slow).abs() < 0.01,
            "slow_ema: got {:.4}, expected {:.4}",
            signal.slow_ema,
            expected_slow
        );
    }

    /// Verifies that a sell crossunder fires from equality when price drops.
    ///
    /// Mirror of `test_crossover_from_equality`: constant 100, then drop to 98.
    /// EMA12 drops faster than EMA26 => crossunder.
    #[test]
    fn test_crossunder_from_equality() {
        let mut closes = vec![100.0; 52];
        closes.push(98.0);

        let candles = make_candles(&closes);
        let signal = analyze("TEST-EQ-SELL", &candles)
            .expect("expected sell signal when crossing under from equality");

        assert_eq!(signal.signal_type, SignalType::Sell);
    }

    /// No signal should fire when data is flat (EMAs remain equal, no cross).
    /// This ensures the equality case alone does not trigger a signal without
    /// an actual divergence on the current bar.
    #[test]
    fn test_no_signal_on_prolonged_equality() {
        // 60 bars all at 100.0 — EMAs stay equal forever
        let candles = make_candles(&vec![100.0; 60]);
        assert!(
            analyze("TEST-FLAT", &candles).is_none(),
            "flat data should not produce a signal"
        );
    }

    /// Zone should be Bull when the crossover spike puts close above both EMAs.
    #[test]
    fn test_crossover_zone_is_bull_when_close_above_fast() {
        // Use a larger spike so close > fast_ema > slow_ema => Bull zone
        let mut closes = vec![100.0; 52];
        closes.push(110.0); // large spike

        let candles = make_candles(&closes);
        let signal = analyze("TEST-ZONE", &candles).expect("expected signal");
        assert_eq!(signal.signal_type, SignalType::Buy);
        assert_eq!(signal.zone, Zone::Bull);
    }
}
