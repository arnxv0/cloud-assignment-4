use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct RateLimiter {
    windows: Arc<DashMap<String, VecDeque<Instant>>>,
    pub limit: usize,
    pub window: Duration,
}

impl RateLimiter {
    pub fn new(limit: usize, window_secs: u64) -> Self {
        Self {
            windows: Arc::new(DashMap::new()),
            limit,
            window: Duration::from_secs(window_secs),
        }
    }

    pub fn is_allowed(&self, key: &str) -> bool {
        let now = Instant::now();
        let window = self.window;
        let limit = self.limit;

        let mut entry = self.windows.entry(key.to_string()).or_default();

        while entry
            .front()
            .map_or(false, |&t| now.duration_since(t) >= window)
        {
            entry.pop_front();
        }

        if entry.len() >= limit {
            false
        } else {
            entry.push_back(now);
            true
        }
    }

    pub fn remaining(&self, key: &str) -> usize {
        let now = Instant::now();
        let window = self.window;

        let mut entry = self.windows.entry(key.to_string()).or_default();
        while entry
            .front()
            .map_or(false, |&t| now.duration_since(t) >= window)
        {
            entry.pop_front();
        }

        self.limit.saturating_sub(entry.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_exactly_up_to_limit() {
        let rl = RateLimiter::new(3, 60);
        assert!(rl.is_allowed("192.168.1.1"));
        assert!(rl.is_allowed("192.168.1.1"));
        assert!(rl.is_allowed("192.168.1.1"));
        assert!(!rl.is_allowed("192.168.1.1"));
    }

    #[test]
    fn different_ips_are_tracked_independently() {
        let rl = RateLimiter::new(1, 60);
        assert!(rl.is_allowed("10.0.0.1"));
        assert!(!rl.is_allowed("10.0.0.1"));
        assert!(rl.is_allowed("10.0.0.2"));
    }

    #[test]
    fn remaining_decrements_correctly() {
        let rl = RateLimiter::new(5, 60);
        assert_eq!(rl.remaining("10.0.0.3"), 5);
        rl.is_allowed("10.0.0.3");
        assert_eq!(rl.remaining("10.0.0.3"), 4);
        rl.is_allowed("10.0.0.3");
        assert_eq!(rl.remaining("10.0.0.3"), 3);
    }
}
