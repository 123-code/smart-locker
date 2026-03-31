use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;

const MAX_FAILURES: usize = 5;
const WINDOW_MINUTES: i64 = 15;

pub fn check_rate_limit(
    limiter: &DashMap<String, Vec<DateTime<Utc>>>,
    locker_id: &str,
) -> bool {
    let cutoff = Utc::now() - Duration::minutes(WINDOW_MINUTES);
    let mut entry = limiter.entry(locker_id.to_string()).or_default();
    entry.retain(|t| *t > cutoff);
    entry.len() < MAX_FAILURES
}

pub fn record_failure(
    limiter: &DashMap<String, Vec<DateTime<Utc>>>,
    locker_id: &str,
) {
    let mut entry = limiter.entry(locker_id.to_string()).or_default();
    entry.push(Utc::now());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_rate_limit_allows_requests_under_limit() {
        let limiter = DashMap::new();
        
        // Should allow when no failures recorded
        assert!(check_rate_limit(&limiter, "locker1"));
    }

    #[test]
    fn test_check_rate_limit_blocks_after_max_failures() {
        let limiter = DashMap::new();
        
        // Record MAX_FAILURES failures
        for _ in 0..MAX_FAILURES {
            record_failure(&limiter, "locker2");
        }
        
        // Should now be blocked
        assert!(!check_rate_limit(&limiter, "locker2"));
    }

    #[test]
    fn test_check_rate_limit_tracks_per_locker() {
        let limiter = DashMap::new();
        
        // Record failures for locker1 only
        for _ in 0..MAX_FAILURES {
            record_failure(&limiter, "locker1");
        }
        
        // locker1 should be blocked
        assert!(!check_rate_limit(&limiter, "locker1"));
        
        // locker2 should still be allowed
        assert!(check_rate_limit(&limiter, "locker2"));
    }

    #[test]
    fn test_record_failure_increments_counter() {
        let limiter = DashMap::new();
        
        assert!(check_rate_limit(&limiter, "locker3"));
        
        record_failure(&limiter, "locker3");
        assert!(check_rate_limit(&limiter, "locker3")); // Still under limit
        
        // Fill up to limit
        for _ in 0..(MAX_FAILURES - 1) {
            record_failure(&limiter, "locker3");
        }
        
        assert!(!check_rate_limit(&limiter, "locker3")); // Now blocked
    }
}
