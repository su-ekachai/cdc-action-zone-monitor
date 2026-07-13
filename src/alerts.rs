//! Alert delivery mechanisms.
//!
//! Supports Telegram (Bot API `sendMessage`) and Discord (webhooks). Channels
//! share the plain-text alert content built here and apply their own markup.

pub mod discord;
pub mod telegram;

use crate::signals::{Signal, SignalType};
use crate::state::LastSignal;

/// Builds the tracking header prepended to every outgoing message:
/// `📅 Daily Scan · YYYY-MM-DD` (UTC date of the sending scan).
pub(crate) fn date_header() -> String {
    format!(
        "\u{1f4c5} Daily Scan \u{00b7} {}",
        time::OffsetDateTime::now_utc().date()
    )
}

/// Builds the channel-agnostic alert content for a signal.
///
/// Returns `(headline, body)` as plain text. Each channel wraps the headline
/// in its own bold markup (HTML for Telegram, Markdown for Discord).
pub(crate) fn alert_content(signal: &Signal, prior: Option<&LastSignal>) -> (String, String) {
    let emoji = match signal.signal_type {
        SignalType::Buy => "\u{1f7e2}",
        SignalType::Sell => "\u{1f534}",
    };

    let headline = format!("{emoji} {} Signal: {}", signal.signal_type, signal.symbol);

    let trend_label = if signal.price > signal.trend_sma50 {
        "Bullish (above SMA50)"
    } else {
        "Bearish (below SMA50)"
    };

    let mut body = format!(
        "Price: {price:.2}\n\
         Zone: {zone}\n\
         \n\
         Strength:\n\
         \u{2022} RSI(14): {rsi:.1}\n\
         \u{2022} Volume: {vol:.1}x average\n\
         \u{2022} Trend: {trend}\n\
         \n\
         EMA(12): {fast:.2} crossed {direction} EMA(26): {slow:.2}",
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
    );

    if let Some(link) = linked_line(signal, prior) {
        body.push_str("\n\n");
        body.push_str(&link);
    }

    (headline, body)
}

/// Formats the linked-signal context: how price moved since the previous
/// opposite signal for the same symbol.
///
/// A SELL links back to the BUY it closes ("Result" = the round-trip move);
/// a BUY links back to the prior SELL ("Since exit" = the move while flat).
/// Returns `None` on the first-ever signal, a same-direction prior (possible
/// after missed opposite crossovers during downtime), or unparseable state.
fn linked_line(signal: &Signal, prior: Option<&LastSignal>) -> Option<String> {
    let prior = prior?;
    if prior.signal_type == signal.signal_type || prior.price <= 0.0 {
        return None;
    }

    let pct = (signal.price - prior.price) / prior.price * 100.0;
    let days = days_between(&prior.date, signal.timestamp)?;
    let label = match signal.signal_type {
        SignalType::Sell => "Result",
        SignalType::Buy => "Since exit",
    };

    Some(format!(
        "\u{21a9} Linked: {} @{:.2} on {}\n{label}: {pct:+.1}% over {days} day(s)",
        prior.signal_type, prior.price, prior.date
    ))
}

/// Days elapsed from an ISO `YYYY-MM-DD` date to a unix timestamp's UTC date.
fn days_between(date: &str, timestamp: i64) -> Option<i64> {
    let mut parts = date.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;

    let prior_date =
        time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()?;
    let signal_date = time::OffsetDateTime::from_unix_timestamp(timestamp)
        .ok()?
        .date();

    Some((signal_date - prior_date).whole_days())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::{Signal, SignalType, Zone};

    const DAY: i64 = 86_400;
    /// 2024-01-01T00:00:00Z
    const JAN1: i64 = 1_704_067_200;

    fn make_signal(signal_type: SignalType, price: f64) -> Signal {
        Signal {
            symbol: "TSLA".to_string(),
            signal_type,
            zone: Zone::Bull,
            price,
            fast_ema: 243.50,
            slow_ema: 241.20,
            rsi: 62.3,
            volume_ratio: 1.8,
            trend_sma50: 230.0,
            timestamp: JAN1 + 72 * DAY,
        }
    }

    fn make_prior(signal_type: SignalType, price: f64) -> LastSignal {
        LastSignal {
            signal_type,
            zone: Zone::Bull,
            date: "2024-01-01".to_string(),
            price,
        }
    }

    #[test]
    fn test_linked_line_sell_after_buy_gain() {
        let signal = make_signal(SignalType::Sell, 260.10);
        let prior = make_prior(SignalType::Buy, 245.67);

        let line = linked_line(&signal, Some(&prior)).unwrap();
        assert!(line.contains("Linked: BUY @245.67 on 2024-01-01"));
        assert!(line.contains("Result: +5.9% over 72 day(s)"));
    }

    #[test]
    fn test_linked_line_sell_after_buy_loss() {
        let signal = make_signal(SignalType::Sell, 200.00);
        let prior = make_prior(SignalType::Buy, 250.00);

        let line = linked_line(&signal, Some(&prior)).unwrap();
        assert!(line.contains("Result: -20.0%"));
    }

    #[test]
    fn test_linked_line_buy_after_sell() {
        let signal = make_signal(SignalType::Buy, 210.00);
        let prior = make_prior(SignalType::Sell, 200.00);

        let line = linked_line(&signal, Some(&prior)).unwrap();
        assert!(line.contains("Linked: SELL @200.00"));
        assert!(line.contains("Since exit: +5.0%"));
    }

    #[test]
    fn test_linked_line_none_without_prior() {
        let signal = make_signal(SignalType::Buy, 210.00);
        assert!(linked_line(&signal, None).is_none());
    }

    #[test]
    fn test_linked_line_none_for_same_direction() {
        let signal = make_signal(SignalType::Buy, 210.00);
        let prior = make_prior(SignalType::Buy, 200.00);
        assert!(linked_line(&signal, Some(&prior)).is_none());
    }

    #[test]
    fn test_linked_line_none_for_bad_date() {
        let signal = make_signal(SignalType::Sell, 210.00);
        let mut prior = make_prior(SignalType::Buy, 200.00);
        prior.date = "not-a-date".to_string();
        assert!(linked_line(&signal, Some(&prior)).is_none());
    }

    #[test]
    fn test_alert_content_includes_linked_line() {
        let signal = make_signal(SignalType::Sell, 260.10);
        let prior = make_prior(SignalType::Buy, 245.67);

        let (headline, body) = alert_content(&signal, Some(&prior));
        assert!(headline.contains("SELL Signal: TSLA"));
        assert!(body.contains("Linked: BUY @245.67"));
    }

    #[test]
    fn test_alert_content_no_linked_line_on_first_signal() {
        let signal = make_signal(SignalType::Buy, 245.67);
        let (_, body) = alert_content(&signal, None);
        assert!(!body.contains("Linked:"));
    }

    #[test]
    fn test_date_header_prefix_and_iso_date() {
        let header = date_header();
        let prefix = "\u{1f4c5} Daily Scan \u{00b7} ";
        assert!(header.starts_with(prefix));
        let date = &header[prefix.len()..];
        assert_eq!(date.len(), 10, "expected YYYY-MM-DD, got {date}");
        assert!(days_between(date, 1_704_067_200).is_some(), "date parses");
    }

    #[test]
    fn test_days_between() {
        assert_eq!(days_between("2024-01-01", JAN1 + 72 * DAY), Some(72));
        assert_eq!(days_between("2024-01-01", JAN1), Some(0));
        assert_eq!(days_between("garbage", JAN1), None);
    }
}
