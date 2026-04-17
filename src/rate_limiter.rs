use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

struct Bucket {
    tokens: u32,
    last_refill: Instant,
}

pub struct RateLimiter {
    max_tokens: u32,
    refill_per_second: f64,
    state: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    pub fn new(max_per_minute: u32) -> Self {
        Self {
            max_tokens: max_per_minute,
            refill_per_second: max_per_minute as f64 / 60.0,
            state: Mutex::new(HashMap::new()),
        }
    }

    pub async fn check(&self, key: &str) -> bool {
        let mut state = self.state.lock().await;
        let now = Instant::now();

        let bucket = state.entry(key.to_string()).or_insert(Bucket {
            tokens: self.max_tokens,
            last_refill: now,
        });

        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        let refilled = (elapsed * self.refill_per_second) as u32;
        if refilled > 0 {
            bucket.tokens = (bucket.tokens + refilled).min(self.max_tokens);
            bucket.last_refill = now;
        }

        if bucket.tokens > 0 {
            bucket.tokens -= 1;
            true
        } else {
            false
        }
    }

    pub async fn cleanup(&self, max_age_secs: u64) {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        state.retain(|_, bucket| {
            now.duration_since(bucket.last_refill).as_secs() < max_age_secs
        });
    }
}

pub type SharedRateLimiter = Arc<RateLimiter>;
