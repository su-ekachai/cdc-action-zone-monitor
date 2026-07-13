//! Discord webhook alert delivery.
//!
//! Posts Markdown-formatted messages to a Discord webhook URL. Webhooks need
//! no bot token; the URL itself is the credential and is never logged.
//! A 100ms delay follows each call to respect Discord rate limits
//! (30 requests/minute per webhook).

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::signals::Signal;
use crate::state::LastSignal;

/// Verifies a Discord webhook by fetching its metadata (GET does not post).
///
/// Returns the webhook's name on success.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the API returns non-200.
pub fn ping(webhook_url: &str) -> anyhow::Result<String> {
    log::debug!("Pinging Discord webhook (GET metadata)");

    let mut response = crate::http::agent()
        .get(webhook_url)
        .call()
        .context("Discord webhook request failed")?;

    if response.status() != 200 {
        anyhow::bail!("Discord returned HTTP {}", response.status());
    }

    #[derive(Deserialize)]
    struct WebhookInfo {
        name: Option<String>,
    }

    let body: WebhookInfo = response
        .body_mut()
        .read_json()
        .context("Failed to parse Discord webhook response")?;

    Ok(body.name.unwrap_or_else(|| "unknown".to_string()))
}

#[derive(Serialize)]
struct DiscordMessage {
    content: String,
}

/// Builds the Markdown-formatted alert message for a signal.
pub(crate) fn format_alert_message(signal: &Signal, prior: Option<&LastSignal>) -> String {
    let (headline, body) = crate::alerts::alert_content(signal, prior);
    format!("**{headline}**\n{body}")
}

/// Sends a formatted signal alert to the configured Discord webhook.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the API returns a non-2xx status.
pub fn send_alert(
    webhook_url: &str,
    signal: &Signal,
    prior: Option<&LastSignal>,
) -> anyhow::Result<()> {
    let msg = DiscordMessage {
        content: format_alert_message(signal, prior),
    };

    log::debug!("{}: sending discord alert", signal.symbol);

    let response = crate::http::agent()
        .post(webhook_url)
        .send_json(&msg)
        .with_context(|| format!("{}: discord webhook request failed", signal.symbol))?;

    // Rate-limit courtesy: sleep after each request regardless of outcome.
    // ureq v3 converts 4xx/5xx to Err at send_json(); Discord replies
    // 204 No Content on success, so accept any 2xx here.
    std::thread::sleep(std::time::Duration::from_millis(100));

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        anyhow::bail!(
            "{}: discord returned HTTP {status} — alert not delivered",
            signal.symbol
        );
    }

    log::debug!("{}: discord alert delivered (HTTP {status})", signal.symbol);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::{SignalType, Zone};

    fn make_signal(signal_type: SignalType) -> Signal {
        Signal {
            symbol: "TSLA".to_string(),
            signal_type,
            zone: Zone::Bull,
            price: 245.67,
            fast_ema: 243.50,
            slow_ema: 241.20,
            rsi: 62.3,
            volume_ratio: 1.8,
            trend_sma50: 230.0,
            timestamp: 1_704_067_200,
        }
    }

    #[test]
    fn test_format_alert_bold_headline_markdown() {
        let msg = format_alert_message(&make_signal(SignalType::Buy), None);

        assert!(msg.starts_with("**"));
        assert!(msg.contains("BUY Signal: TSLA**"));
        assert!(msg.contains("Price: 245.67"));
        assert!(msg.contains("EMA(12): 243.50"));
        // No Telegram HTML markup leaks into Discord output.
        assert!(!msg.contains("<b>"));
    }

    #[test]
    fn test_format_alert_includes_linked_signal() {
        let prior = LastSignal {
            signal_type: SignalType::Sell,
            zone: Zone::Bear,
            date: "2024-01-01".to_string(),
            price: 200.0,
        };
        let msg = format_alert_message(&make_signal(SignalType::Buy), Some(&prior));

        assert!(msg.contains("Linked: SELL @200.00 on 2024-01-01"));
    }
}
