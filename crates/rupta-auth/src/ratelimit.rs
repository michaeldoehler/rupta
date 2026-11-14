//! Protection of the login against password guessing.
//!
//! Failed logins are counted per account (normalized email) and per client
//! IP in a sliding window. An account is locked for 15 minutes after 10
//! failures within 15 minutes; an IP after 50. While locked, logins are
//! refused before the password is checked, also with the right password.
//! Unknown emails are counted like real ones, so the answers do not reveal
//! which accounts exist. The state is kept in memory: it resets when the
//! server restarts.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct Limit {
    pub failures: usize,
    pub window: Duration,
    pub lockout: Duration,
}

pub const ACCOUNT_LIMIT: Limit =
    Limit { failures: 10, window: Duration::from_secs(15 * 60), lockout: Duration::from_secs(15 * 60) };
pub const IP_LIMIT: Limit =
    Limit { failures: 50, window: Duration::from_secs(15 * 60), lockout: Duration::from_secs(15 * 60) };

#[derive(Default)]
struct Entry {
    failures: VecDeque<Instant>,
    locked_until: Option<Instant>,
}

/// Counters for one kind of key.
pub struct Limiter {
    limit: Limit,
    entries: Mutex<HashMap<String, Entry>>,
}

impl Limiter {
    pub fn new(limit: Limit) -> Limiter {
        Limiter { limit, entries: Mutex::new(HashMap::new()) }
    }

    /// `Err(retry_after)` while `key` is locked.
    pub fn check(&self, key: &str) -> Result<(), Duration> {
        self.check_at(key, Instant::now())
    }

    fn check_at(&self, key: &str, now: Instant) -> Result<(), Duration> {
        let entries = self.entries.lock().unwrap();
        match entries.get(key).and_then(|e| e.locked_until) {
            Some(until) if until > now => Err(until - now),
            _ => Ok(()),
        }
    }

    /// Counts a failure; returns true if the key is now locked.
    pub fn failure(&self, key: &str) -> bool {
        self.failure_at(key, Instant::now())
    }

    fn failure_at(&self, key: &str, now: Instant) -> bool {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() > 100_000 {
            let window = self.limit.window;
            entries.retain(|_, e| {
                e.locked_until.is_some_and(|u| u > now) || e.failures.back().is_some_and(|t| now - *t < window)
            });
        }
        let e = entries.entry(key.to_string()).or_default();
        while e.failures.front().is_some_and(|t| now.duration_since(*t) >= self.limit.window) {
            e.failures.pop_front();
        }
        e.failures.push_back(now);
        if e.failures.len() >= self.limit.failures {
            e.locked_until = Some(now + self.limit.lockout);
            e.failures.clear();
            return true;
        }
        false
    }

    /// Forgets the failures of a key after a successful login.
    pub fn success(&self, key: &str) {
        self.entries.lock().unwrap().remove(key);
    }
}

/// Login protection by account and by IP.
pub struct LoginGuard {
    pub accounts: Limiter,
    pub ips: Limiter,
}

impl Default for LoginGuard {
    fn default() -> LoginGuard {
        LoginGuard { accounts: Limiter::new(ACCOUNT_LIMIT), ips: Limiter::new(IP_LIMIT) }
    }
}

impl LoginGuard {
    /// `Err(retry_after)` if the account or the IP is locked.
    pub fn check(&self, email: &str, ip: &str) -> Result<(), Duration> {
        self.accounts.check(&email.to_lowercase())?;
        self.ips.check(ip)
    }

    /// Returns true if this failure locked the account or the IP.
    pub fn failure(&self, email: &str, ip: &str) -> bool {
        let a = self.accounts.failure(&email.to_lowercase());
        let i = self.ips.failure(ip);
        a || i
    }

    pub fn success(&self, email: &str) {
        self.accounts.success(&email.to_lowercase());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_after_too_many_failures_and_unlocks_later() {
        let l = Limiter::new(Limit { failures: 3, window: Duration::from_secs(60), lockout: Duration::from_secs(30) });
        let t0 = Instant::now();
        assert!(!l.failure_at("a", t0));
        assert!(!l.failure_at("a", t0));
        assert!(l.check_at("a", t0).is_ok());
        assert!(l.failure_at("a", t0));
        assert!(l.check_at("a", t0 + Duration::from_secs(10)).is_err());
        assert!(l.check_at("b", t0).is_ok());
        assert!(l.check_at("a", t0 + Duration::from_secs(31)).is_ok());
    }

    #[test]
    fn old_failures_leave_the_window() {
        let l = Limiter::new(Limit { failures: 3, window: Duration::from_secs(60), lockout: Duration::from_secs(30) });
        let t0 = Instant::now();
        l.failure_at("a", t0);
        l.failure_at("a", t0);
        assert!(!l.failure_at("a", t0 + Duration::from_secs(61)));
    }

    #[test]
    fn success_resets() {
        let g = LoginGuard::default();
        for _ in 0..9 {
            g.failure("A@x.org", "1.1.1.1");
        }
        g.success("a@X.org");
        assert!(!g.failure("a@x.org", "1.1.1.1"));
        assert!(g.check("a@x.org", "1.1.1.1").is_ok());
    }
}
