//! Shared HTTP agent construction.

use std::time::Duration;

/// Builds a `ureq` agent with explicit timeouts.
///
/// `ureq`'s default config has no timeouts at all, so a hung connection would
/// block the cron job indefinitely on a flaky network.
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .timeout_connect(Some(Duration::from_secs(10)))
        .build()
        .new_agent()
}
