//! Shared HTTP agent construction.

use std::sync::OnceLock;
use std::time::Duration;

/// Returns a clone of the process-wide `ureq` agent.
///
/// The agent is built once (TLS root-store parsing is not free) and cloned
/// per caller; clones are `Arc`-backed and share one connection pool, so
/// repeated requests to the same host reuse connections.
///
/// Explicit timeouts because `ureq`'s default config has none at all — a hung
/// connection would block the cron job indefinitely on a flaky network.
pub fn agent() -> ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(30)))
                .timeout_connect(Some(Duration::from_secs(10)))
                .build()
                .new_agent()
        })
        .clone()
}
