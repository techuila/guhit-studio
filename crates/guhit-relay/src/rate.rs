//! The rate limit of one direction of a pair.

use std::time::Duration;

use tokio::time::Instant;

/// A token bucket: `rate` bytes a second, with up to `burst` saved up. A
/// frame larger than what is saved still passes: the bucket goes into debt,
/// and the frame waits until the debt is paid.
pub(crate) struct Bucket {
    rate: f64,
    burst: f64,
    tokens: f64,
    at: Instant,
}

impl Bucket {
    /// `rate` 0: no limit.
    pub(crate) fn new(rate: u64, burst: u64) -> Bucket {
        Bucket { rate: rate as f64, burst: burst as f64, tokens: burst as f64, at: Instant::now() }
    }

    /// Takes `bytes` out, and answers how long to wait before forwarding
    /// them. None: forward now.
    pub(crate) fn take(&mut self, bytes: usize) -> Option<Duration> {
        if self.rate <= 0.0 {
            return None;
        }
        let now = Instant::now();
        let earned = now.saturating_duration_since(self.at).as_secs_f64() * self.rate;
        self.tokens = (self.tokens + earned).min(self.burst);
        self.at = now;
        self.tokens -= bytes as f64;
        if self.tokens >= 0.0 {
            return None;
        }
        // Past the burst: wait until the rate has paid the debt back.
        Some(Duration::try_from_secs_f64(-self.tokens / self.rate).unwrap_or(Duration::MAX))
    }
}
