//! Rate limits for what the launcher sends out: a token bucket per service, so bursts are
//! fine but a sustained rate stays under the service's limit (Modrinth: 300 requests/min).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

pub struct Bucket {
    capacity: f64,
    per_second: f64,
    state: Mutex<(f64, Instant)>,
}

impl Bucket {
    pub const fn new(capacity: f64, per_second: f64, now: Instant) -> Self {
        Self { capacity, per_second, state: Mutex::new((capacity, now)) }
    }

    /// Time to wait before the next request may go out (and reserves it).
    fn reserve(&self) -> Duration {
        let mut s = self.state.lock().unwrap();
        let now = Instant::now();
        let refill = now.duration_since(s.1).as_secs_f64() * self.per_second;
        s.0 = (s.0 + refill).min(self.capacity);
        s.1 = now;
        s.0 -= 1.0;
        if s.0 >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-s.0 / self.per_second)
        }
    }

    pub async fn take(&self) {
        let wait = self.reserve();
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    /// Non-waiting check for user actions (sign-in attempts): false = over the limit.
    pub fn try_take(&self) -> bool {
        let mut s = self.state.lock().unwrap();
        let now = Instant::now();
        let refill = now.duration_since(s.1).as_secs_f64() * self.per_second;
        s.0 = (s.0 + refill).min(self.capacity);
        s.1 = now;
        if s.0 >= 1.0 {
            s.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Modrinth API (not the CDN): bursts of 20, then 4/s = 240/min, under their 300/min.
pub static MODRINTH: LazyLock<Bucket> = LazyLock::new(|| Bucket::new(20.0, 4.0, Instant::now()));

/// Sign-in attempts per CSCM machine: 5 per minute.
static LOGINS: LazyLock<Mutex<HashMap<String, std::sync::Arc<Bucket>>>> = LazyLock::new(Default::default);

pub fn login_allowed(target: &str) -> bool {
    let bucket = LOGINS
        .lock()
        .unwrap()
        .entry(target.to_string())
        .or_insert_with(|| std::sync::Arc::new(Bucket::new(5.0, 5.0 / 60.0, Instant::now())))
        .clone();
    bucket.try_take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bursts_then_waits() {
        let b = Bucket::new(3.0, 1.0, Instant::now());
        assert!(b.reserve().is_zero() && b.reserve().is_zero() && b.reserve().is_zero());
        let wait = b.reserve();
        assert!(wait > Duration::from_millis(900) && wait <= Duration::from_secs(1), "{wait:?}");
    }

    #[test]
    fn logins_are_limited_per_target() {
        for _ in 0..5 {
            assert!(login_allowed("http://a"));
        }
        assert!(!login_allowed("http://a"));
        assert!(login_allowed("http://b"));
    }
}
