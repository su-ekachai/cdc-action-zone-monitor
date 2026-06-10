//! Telegram Bot API alert delivery.
//!
//! Sends HTML-formatted messages via the `sendMessage` endpoint.
//! A 100ms delay follows each call to respect Telegram rate limits
//! (30 messages/second per bot, 20 messages/minute per group).

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::signals::{Signal, SignalType};

/// Verifies a Telegram bot token by calling the `getMe` endpoint.
///
/// Returns the bot's username on success.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the API returns non-200.
pub fn ping(token: &str) -> anyhow::Result<String> {
    let url = format!("https://api.telegram.org/bot{token}/getMe");
    log::debug!("Pinging Telegram API (getMe)");

    let mut response = crate::http::agent()
        .get(&url)
        .call()
        .context("Telegram API request failed")?;

    if response.status() != 200 {
        anyhow::bail!("Telegram returned HTTP {}", response.status());
    }

    #[derive(Deserialize)]
    struct GetMeResponse {
        result: GetMeResult,
    }
    #[derive(Deserialize)]
    struct GetMeResult {
        username: Option<String>,
    }

    let body: GetMeResponse = response
        .body_mut()
        .read_json()
        .context("Failed to parse Telegram getMe response")?;

    Ok(body
        .result
        .username
        .unwrap_or_else(|| "unknown".to_string()))
}

#[derive(Serialize)]
struct TelegramMessage {
    chat_id: String,
    text: String,
    parse_mode: String,
}

/// Escapes the HTML-special characters required by Telegram's HTML parse mode.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Builds the HTML-formatted alert message for a signal.
pub(crate) fn format_alert_message(signal: &Signal) -> String {
    let emoji = match signal.signal_type {
        SignalType::Buy => "\u{1f7e2}",
        SignalType::Sell => "\u{1f534}",
    };

    let trend_label = if signal.price > signal.trend_sma50 {
        "Bullish (above SMA50)"
    } else {
        "Bearish (below SMA50)"
    };

    format!(
        "<b>{emoji} {signal_type} Signal: {symbol}</b>\n\
         Price: {price:.2}\n\
         Zone: {zone}\n\
         \n\
         Strength:\n\
         \u{2022} RSI(14): {rsi:.1}\n\
         \u{2022} Volume: {vol:.1}x average\n\
         \u{2022} Trend: {trend}\n\
         \n\
         EMA(12): {fast:.2} crossed {direction} EMA(26): {slow:.2}",
        signal_type = signal.signal_type,
        symbol = escape_html(&signal.symbol),
        price = signal.price,
        zone = signal.zone.label(),
        rsi = signal.rsi,
        vol = signal.volume_ratio,
        trend = trend_label,
        fast = signal.fast_ema,
        slow = signal.slow_ema,
        direction = match signal.signal_type {
            SignalType::Buy => "above",
            SignalType::Sell => "below",
        },
    )
}

/// Sends a formatted signal alert to the configured Telegram chat.
///
/// The message includes signal direction, price, zone, and strength indicators
/// (RSI, volume ratio, trend) formatted as HTML.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the API returns non-200.
pub fn send_alert(config: &Config, signal: &Signal) -> anyhow::Result<()> {
    let text = format_alert_message(signal);

    let url = format!(
        "https://api.telegram.org/bot{}/sendMessage",
        config.telegram_bot_token
    );

    let msg = TelegramMessage {
        chat_id: config.telegram_chat_id.clone(),
        text,
        parse_mode: "HTML".to_string(),
    };

    log::debug!(
        "{}: sending telegram alert to chat {}",
        signal.symbol,
        config.telegram_chat_id
    );

    let response = crate::http::agent()
        .post(&url)
        .send_json(&msg)
        .with_context(|| format!("{}: telegram API request failed", signal.symbol))?;

    // Rate-limit courtesy: sleep after each request regardless of outcome.
    // Note: ureq v3 converts 4xx/5xx to Err at send_json(), so the status
    // check below is defense-in-depth for non-standard success codes (1xx/3xx).
    std::thread::sleep(std::time::Duration::from_millis(100));

    if response.status() != 200 {
        anyhow::bail!(
            "{}: telegram returned HTTP {} — alert not delivered",
            signal.symbol,
            response.status()
        );
    }

    log::debug!("{}: telegram alert delivered (HTTP 200)", signal.symbol);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::{Signal, SignalType, Zone};

    fn make_signal(signal_type: SignalType, price: f64, trend_sma50: f64) -> Signal {
        Signal {
            symbol: "TSLA".to_string(),
            signal_type,
            zone: Zone::Bull,
            price,
            fast_ema: 243.50,
            slow_ema: 241.20,
            rsi: 62.3,
            volume_ratio: 1.8,
            trend_sma50,
            timestamp: 1_704_067_200,
        }
    }

    #[test]
    fn test_format_alert_buy_signal() {
        let signal = make_signal(SignalType::Buy, 245.67, 230.0);
        let msg = format_alert_message(&signal);

        assert!(msg.contains("\u{1f7e2}"));
        assert!(msg.contains("BUY Signal: TSLA"));
        assert!(msg.contains("Price: 245.67"));
        assert!(msg.contains("crossed above"));
        assert!(msg.contains("EMA(12): 243.50"));
        assert!(msg.contains("EMA(26): 241.20"));
    }

    #[test]
    fn test_format_alert_sell_signal() {
        let signal = make_signal(SignalType::Sell, 200.00, 230.0);
        let msg = format_alert_message(&signal);

        assert!(msg.contains("\u{1f534}"));
        assert!(msg.contains("SELL Signal: TSLA"));
        assert!(msg.contains("crossed below"));
    }

    #[test]
    fn test_format_alert_bullish_trend() {
        let signal = make_signal(SignalType::Buy, 250.0, 230.0);
        let msg = format_alert_message(&signal);

        assert!(msg.contains("Bullish (above SMA50)"));
    }

    #[test]
    fn test_format_alert_bearish_trend() {
        let signal = make_signal(SignalType::Buy, 220.0, 230.0);
        let msg = format_alert_message(&signal);

        assert!(msg.contains("Bearish (below SMA50)"));
    }

    #[test]
    fn test_format_alert_escapes_html_in_symbol() {
        let mut signal = make_signal(SignalType::Buy, 245.67, 230.0);
        signal.symbol = "A&B<C>".to_string();
        let msg = format_alert_message(&signal);

        assert!(msg.contains("A&amp;B&lt;C&gt;"));
        assert!(!msg.contains("B<C>"));
    }

    #[test]
    fn test_format_alert_contains_strength_indicators() {
        let signal = make_signal(SignalType::Buy, 245.67, 230.0);
        let msg = format_alert_message(&signal);

        assert!(msg.contains("RSI(14): 62.3"));
        assert!(msg.contains("Volume: 1.8x average"));
        assert!(msg.contains("Zone: Bull"));
    }
}
