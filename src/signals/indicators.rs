//! Technical indicator calculations for time-series price and volume data.
//!
//! All functions accept a `&[f64]` slice and return a `Vec<f64>` of equal length.
//! Indices with insufficient warm-up data are filled with [`f64::NAN`].
//! Output vectors align positionally with the input candle array.

/// Computes the Simple Moving Average (SMA) over a rolling window.
///
/// The first `period - 1` elements are `NAN` (window not yet filled).
/// Uses an incremental sum approach: O(n) time, O(1) extra space.
pub fn sma(data: &[f64], period: usize) -> Vec<f64> {
    let mut result = vec![f64::NAN; data.len()];
    if data.len() < period {
        return result;
    }
    let mut sum: f64 = data[..period].iter().sum();
    result[period - 1] = sum / period as f64;
    for i in period..data.len() {
        sum += data[i] - data[i - period];
        result[i] = sum / period as f64;
    }
    result
}

/// Computes the Exponential Moving Average (EMA) with SMA seeding.
///
/// The seed (index `period - 1`) is the simple average of the first `period` elements.
/// Subsequent values: `EMA = (value - prev) * k + prev` where `k = 2 / (period + 1)`.
/// Indices before the seed are `NAN`.
pub fn ema(data: &[f64], period: usize) -> Vec<f64> {
    let mut result = vec![f64::NAN; data.len()];
    if data.len() < period {
        return result;
    }
    let multiplier = 2.0 / (period as f64 + 1.0);
    let seed: f64 = data[..period].iter().sum::<f64>() / period as f64;
    result[period - 1] = seed;
    for i in period..data.len() {
        result[i] = (data[i] - result[i - 1]) * multiplier + result[i - 1];
    }
    result
}

/// Computes the Relative Strength Index (RSI) using Wilder's smoothing method.
///
/// Requires at least `period + 1` data points. The first valid RSI appears at
/// index `period`. Uses exponential smoothing of gains/losses after the initial
/// simple average, matching the standard Wilder/Cutler RSI implementation.
///
/// Output range: 0.0 (all losses) to 100.0 (all gains).
pub fn rsi(close: &[f64], period: usize) -> Vec<f64> {
    let mut result = vec![f64::NAN; close.len()];
    if close.len() < period + 1 {
        return result;
    }

    let mut gains = Vec::with_capacity(close.len());
    let mut losses = Vec::with_capacity(close.len());
    gains.push(0.0);
    losses.push(0.0);
    for i in 1..close.len() {
        let delta = close[i] - close[i - 1];
        gains.push(if delta > 0.0 { delta } else { 0.0 });
        losses.push(if delta < 0.0 { -delta } else { 0.0 });
    }

    let mut avg_gain: f64 = gains[1..=period].iter().sum::<f64>() / period as f64;
    let mut avg_loss: f64 = losses[1..=period].iter().sum::<f64>() / period as f64;

    if avg_loss == 0.0 {
        result[period] = 100.0;
    } else {
        result[period] = 100.0 - (100.0 / (1.0 + avg_gain / avg_loss));
    }

    for i in (period + 1)..close.len() {
        avg_gain = (avg_gain * (period as f64 - 1.0) + gains[i]) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + losses[i]) / period as f64;
        if avg_loss == 0.0 {
            result[i] = 100.0;
        } else {
            result[i] = 100.0 - (100.0 / (1.0 + avg_gain / avg_loss));
        }
    }
    result
}

/// Computes the ratio of each volume bar to its trailing SMA.
///
/// A value of 1.0 indicates average volume; 2.0 means double the recent average.
/// Returns `NAN` where the SMA is unavailable or zero.
pub fn volume_ratio(volume: &[f64], period: usize) -> Vec<f64> {
    let vol_sma = sma(volume, period);
    volume
        .iter()
        .zip(vol_sma.iter())
        .map(|(v, s)| {
            if s.is_nan() || *s == 0.0 {
                f64::NAN
            } else {
                v / s
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sma_basic() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = sma(&data, 3);
        assert!(result[0].is_nan());
        assert!(result[1].is_nan());
        assert!((result[2] - 2.0).abs() < 1e-10);
        assert!((result[3] - 3.0).abs() < 1e-10);
        assert!((result[4] - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_ema_basic() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let result = ema(&data, 3);
        assert!(result[0].is_nan());
        assert!(result[1].is_nan());
        assert!((result[2] - 2.0).abs() < 1e-10);
        assert!((result[3] - 3.0).abs() < 1e-10);
        assert!((result[4] - 4.0).abs() < 1e-10);
    }

    /// Verifies EMA exponential decay when price drops sharply after trend.
    ///
    /// Data: [10, 11, 12, 13, 14, 10, 10, 10, 10, 10] with period=5
    /// The EMA seed (index 4) = SMA([10,11,12,13,14]) = 12.0.
    /// After the drop to 10, EMA decays exponentially toward 10 but never
    /// reaches it instantly. This proves the smoothing works correctly and
    /// would fail if the multiplier were wrong (e.g., multiplier=1 would
    /// make EMA immediately equal the input).
    ///
    /// Hand-calculated with k = 2/(5+1) = 0.333333:
    ///   EMA[5] = (10 - 12) * 0.3333 + 12 = 11.3333
    ///   EMA[6] = (10 - 11.3333) * 0.3333 + 11.3333 = 10.8889
    ///   EMA[7] = (10 - 10.8889) * 0.3333 + 10.8889 = 10.5926
    ///   EMA[8] = (10 - 10.5926) * 0.3333 + 10.5926 = 10.3951
    ///   EMA[9] = (10 - 10.3951) * 0.3333 + 10.3951 = 10.2634
    #[test]
    fn test_ema_exponential_decay_after_price_drop() {
        let data = vec![10.0, 11.0, 12.0, 13.0, 14.0, 10.0, 10.0, 10.0, 10.0, 10.0];
        let result = ema(&data, 5);

        // Seed = SMA of first 5 = 12.0
        assert!((result[4] - 12.0).abs() < 1e-10, "seed should be 12.0");

        // k = 2/(5+1) = 1/3
        let k = 2.0 / 6.0;
        let expected_5 = (10.0 - 12.0) * k + 12.0; // 11.3333...
        let expected_6 = (10.0 - expected_5) * k + expected_5; // 10.8889...
        let expected_7 = (10.0 - expected_6) * k + expected_6; // 10.5926...
        let expected_8 = (10.0 - expected_7) * k + expected_7; // 10.3951...
        let expected_9 = (10.0 - expected_8) * k + expected_8; // 10.2634...

        let tol = 1e-4;
        assert!(
            (result[5] - expected_5).abs() < tol,
            "EMA[5]: got {}, expected {}",
            result[5],
            expected_5
        );
        assert!(
            (result[6] - expected_6).abs() < tol,
            "EMA[6]: got {}, expected {}",
            result[6],
            expected_6
        );
        assert!(
            (result[7] - expected_7).abs() < tol,
            "EMA[7]: got {}, expected {}",
            result[7],
            expected_7
        );
        assert!(
            (result[8] - expected_8).abs() < tol,
            "EMA[8]: got {}, expected {}",
            result[8],
            expected_8
        );
        assert!(
            (result[9] - expected_9).abs() < tol,
            "EMA[9]: got {}, expected {}",
            result[9],
            expected_9
        );

        // Additional structural assertions:
        // EMA should monotonically decrease toward 10 but remain above it
        for (i, val) in result.iter().enumerate().take(10).skip(5) {
            assert!(*val > 10.0, "EMA[{i}] should be above 10.0, got {val}");
        }
        for window in result[5..10].windows(2) {
            assert!(
                window[0] > window[1],
                "EMA should be monotonically decreasing: {} > {}",
                window[0],
                window[1]
            );
        }
    }

    /// Verifies EMA multiplier formula: k = 2 / (period + 1).
    ///
    /// Uses period=10, a single step from 0 to 100 after seeding at 0.
    /// The EMA at the step should equal exactly k * 100 = 2/11 * 100 = 18.1818...
    #[test]
    fn test_ema_multiplier_correctness() {
        // 10 zeros (SMA seed = 0), then one 100
        let mut data = vec![0.0; 10];
        data.push(100.0);
        let result = ema(&data, 10);

        let k = 2.0 / 11.0; // 0.18181818...
        let expected = k * 100.0; // 18.1818...
        assert!(
            (result[10] - expected).abs() < 1e-10,
            "EMA[10] should be {expected}, got {}",
            result[10]
        );
    }

    #[test]
    fn test_rsi_overbought() {
        let data: Vec<f64> = (0..20).map(|i| 100.0 + f64::from(i)).collect();
        let result = rsi(&data, 14);
        assert!(result[14] > 90.0);
    }

    #[test]
    fn test_rsi_oversold() {
        let data: Vec<f64> = (0..20).map(|i| 100.0 - f64::from(i)).collect();
        let result = rsi(&data, 14);
        assert!(result[14] < 10.0);
    }

    /// Tests RSI at a mid-range value using hand-calculated reference data.
    ///
    /// Data: 15 points with alternating +1/-1 moves (8 gains, 6 losses).
    ///   Changes: [+1, +1, -1, -1, +1, +1, +1, -1, -1, +1, +1, +1, -1, -1]
    ///   Avg gain = 8/14 = 0.571429
    ///   Avg loss = 6/14 = 0.428571
    ///   RS = 0.571429 / 0.428571 = 1.333333
    ///   RSI = 100 - 100/(1 + 1.333333) = 100 - 42.857 = 57.143
    #[test]
    fn test_rsi_mid_range_first_period() {
        let data = vec![
            50.0, 51.0, 52.0, 51.0, 50.0, 51.0, 52.0, 53.0, 52.0, 51.0, 52.0, 53.0, 54.0, 53.0,
            52.0,
        ];
        let result = rsi(&data, 14);

        // First valid RSI at index 14
        let expected_rsi = 100.0 - 100.0 / (1.0 + (8.0 / 14.0) / (6.0 / 14.0));
        // = 100 - 100/(1 + 1.3333) = 100 - 42.857 = 57.143

        assert!(!result[14].is_nan(), "RSI[14] should be valid, got NaN");
        assert!(
            (result[14] - expected_rsi).abs() < 0.01,
            "RSI[14]: got {:.4}, expected {:.4}",
            result[14],
            expected_rsi
        );
    }

    /// Tests RSI with Wilder's smoothing on the second period (bar 15+).
    ///
    /// Extends the mid-range test by one data point (53 -> 53, i.e., +1 gain).
    /// Wilder's smoothing: `avg_gain = (prev_avg_gain * 13 + current_gain) / 14`
    ///   Avg gain = (0.571429 * 13 + 1) / 14 = 8.4286/14 = 0.6020
    ///   Avg loss = (0.428571 * 13 + 0) / 14 = 5.5714/14 = 0.3980
    ///   RS = 0.6020 / 0.3980 = 1.5126
    ///   RSI = 100 - 100/(1 + 1.5126) = 60.20
    #[test]
    fn test_rsi_wilders_smoothing_second_bar() {
        let data = vec![
            50.0, 51.0, 52.0, 51.0, 50.0, 51.0, 52.0, 53.0, 52.0, 51.0, 52.0, 53.0, 54.0, 53.0,
            52.0, 53.0,
        ];
        let result = rsi(&data, 14);

        // After Wilder's smoothing for bar 15 (index 15):
        let initial_avg_gain = 8.0 / 14.0; // 0.571429
        let initial_avg_loss = 6.0 / 14.0; // 0.428571

        // data[15] = 53, data[14] = 52 => gain = 1, loss = 0
        let smoothed_avg_gain = (initial_avg_gain * 13.0 + 1.0) / 14.0;
        let smoothed_avg_loss = (initial_avg_loss * 13.0 + 0.0) / 14.0;
        let rs = smoothed_avg_gain / smoothed_avg_loss;
        let expected_rsi = 100.0 - 100.0 / (1.0 + rs);

        assert!(!result[15].is_nan(), "RSI[15] should be valid, got NaN");
        assert!(
            (result[15] - expected_rsi).abs() < 0.01,
            "RSI[15]: got {:.4}, expected {:.4} (Wilder's smoothing)",
            result[15],
            expected_rsi
        );
    }

    /// RSI with zero losses after Wilder's smoothing should approach 100
    /// but not necessarily equal it immediately due to smoothed `avg_loss` > 0.
    #[test]
    fn test_rsi_smoothing_approaches_extreme() {
        // 15 consecutive gains (RSI[14] = 100), then more gains
        let data: Vec<f64> = (0..20).map(|i| 100.0 + f64::from(i)).collect();
        let result = rsi(&data, 14);

        // All gains, no losses => RSI stays at 100
        for (i, val) in result.iter().enumerate().take(20).skip(14) {
            assert!(
                (*val - 100.0).abs() < 1e-10,
                "RSI[{i}]: got {val}, expected 100.0",
            );
        }
    }

    #[test]
    fn test_volume_ratio() {
        let volume = vec![100.0, 100.0, 100.0, 200.0];
        let result = volume_ratio(&volume, 3);
        assert!(result[0].is_nan());
        assert!(result[1].is_nan());
        assert!((result[2] - 1.0).abs() < 1e-10);
        assert!((result[3] - 1.5).abs() < 1e-10);
    }

    #[test]
    fn test_insufficient_data() {
        let data = vec![1.0, 2.0];
        assert!(sma(&data, 5).iter().all(|x| x.is_nan()));
        assert!(ema(&data, 5).iter().all(|x| x.is_nan()));
        assert!(rsi(&data, 14).iter().all(|x| x.is_nan()));
    }

    /// EMA with all identical values should remain constant (no drift).
    #[test]
    fn test_ema_constant_input_no_drift() {
        let data = vec![42.0; 100];
        let result = ema(&data, 12);

        for (i, val) in result.iter().enumerate().take(100).skip(11) {
            assert!(
                (*val - 42.0).abs() < 1e-10,
                "EMA[{i}] drifted from constant: got {val}",
            );
        }
    }

    /// SMA of all-same values should be that same value at every valid index.
    #[test]
    fn test_sma_constant_input() {
        let data = vec![7.5; 50];
        let result = sma(&data, 20);

        for val in result.iter().take(19) {
            assert!(val.is_nan());
        }
        for (i, val) in result.iter().enumerate().take(50).skip(19) {
            assert!(
                (*val - 7.5).abs() < 1e-10,
                "SMA[{i}] should be 7.5, got {val}",
            );
        }
    }
}
