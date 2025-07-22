use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[allow(dead_code)]
pub struct RateLimiter {
    limits: Arc<RwLock<HashMap<String, RateLimit>>>,
    default_limit: u32,
}

#[allow(dead_code)]
struct RateLimit {
    count: u32,
    window_start: Instant,
    max_per_second: u32,
}

#[allow(dead_code)]
impl RateLimiter {
    pub fn new(default_limit: u32) -> Self {
        Self {
            limits: Arc::new(RwLock::new(HashMap::new())),
            default_limit,
        }
    }

    pub async fn check_and_update(&self, endpoint: &str) -> Result<u32, String> {
        let mut limits = self.limits.write().await;
        let now = Instant::now();

        let limit = limits
            .entry(endpoint.to_string())
            .or_insert_with(|| RateLimit {
                count: 0,
                window_start: now,
                max_per_second: self.default_limit,
            });

        // Reset window if more than 1 second has passed
        if now.duration_since(limit.window_start) >= Duration::from_secs(1) {
            limit.count = 0;
            limit.window_start = now;
        }

        // Check if we're within limits
        if limit.count >= limit.max_per_second {
            let remaining_ms = Duration::from_secs(1)
                .saturating_sub(now.duration_since(limit.window_start))
                .as_millis();
            return Err(format!(
                "Rate limit exceeded for {}. Try again in {} ms",
                endpoint, remaining_ms
            ));
        }

        // Update count
        limit.count += 1;
        let remaining = limit.max_per_second - limit.count;

        Ok(remaining)
    }

    pub async fn set_limit(&self, endpoint: &str, max_per_second: u32) {
        let mut limits = self.limits.write().await;
        if let Some(limit) = limits.get_mut(endpoint) {
            limit.max_per_second = max_per_second;
        } else {
            limits.insert(
                endpoint.to_string(),
                RateLimit {
                    count: 0,
                    window_start: Instant::now(),
                    max_per_second,
                },
            );
        }
    }

    pub async fn get_remaining(&self, endpoint: &str) -> u32 {
        let limits = self.limits.read().await;
        if let Some(limit) = limits.get(endpoint) {
            let now = Instant::now();
            if now.duration_since(limit.window_start) >= Duration::from_secs(1) {
                limit.max_per_second
            } else {
                limit.max_per_second.saturating_sub(limit.count)
            }
        } else {
            self.default_limit
        }
    }
}

// Macro for easy rate limiting
#[macro_export]
macro_rules! rate_limit {
    ($limiter:expr, $endpoint:expr, $body:expr) => {{
        match $limiter.check_and_update($endpoint).await {
            Ok(remaining) => {
                if remaining < 10 {
                    $crate::middleware::logging::CommandLogger::log_rate_limit_warning(
                        $endpoint, remaining,
                    );
                }
                $body
            }
            Err(e) => Err(e),
        }
    }};
}