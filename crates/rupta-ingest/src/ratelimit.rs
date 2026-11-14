//! Per-minute rate limits for projects and keys (fixed one-minute
//! windows, counted in memory; a restart resets them).

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Project,
    Key,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Project => "project",
            Scope::Key => "key",
        }
    }
}

const SHARDS: usize = 16;

/// `(scope, id)` → `(minute, count)`.
type Windows = HashMap<(Scope, i64), (i64, u64)>;

pub struct RateLimiter {
    shards: Vec<Mutex<Windows>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        RateLimiter { shards: (0..SHARDS).map(|_| Mutex::new(HashMap::new())).collect() }
    }
}

impl RateLimiter {
    /// Admits `n` items at `now_secs` (Unix seconds) if the minute's count
    /// stays within `limit`; otherwise returns the seconds until the next
    /// window opens.
    pub fn check(&self, scope: Scope, id: i64, limit: u32, n: u64, now_secs: i64) -> Result<(), u64> {
        let minute = now_secs.div_euclid(60);
        let shard = &self.shards[(id as usize ^ scope as usize) % SHARDS];
        let mut map = shard.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() > 100_000 {
            map.retain(|_, (m, _)| *m == minute);
        }
        let entry = map.entry((scope, id)).or_insert((minute, 0));
        if entry.0 != minute {
            *entry = (minute, 0);
        }
        if entry.1 + n > limit as u64 {
            return Err((60 - now_secs.rem_euclid(60)).max(1) as u64);
        }
        entry.1 += n;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_reset_each_minute() {
        let r = RateLimiter::default();
        let t = 1_800_000_010; // 10 s into a minute
        assert!(r.check(Scope::Project, 1, 2, 1, t).is_ok());
        assert!(r.check(Scope::Project, 1, 2, 1, t + 1).is_ok());
        assert_eq!(r.check(Scope::Project, 1, 2, 1, t + 2), Err(48));
        assert!(r.check(Scope::Key, 1, 2, 1, t + 2).is_ok());
        assert!(r.check(Scope::Project, 1, 2, 1, t + 60).is_ok());
    }
}
