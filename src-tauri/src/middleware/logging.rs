use std::time::Instant;
use tracing::{debug, error, info, warn};

#[allow(dead_code)]
pub struct CommandLogger;

#[allow(dead_code)]
impl CommandLogger {
    pub fn log_command_start(command_name: &str, args: &str) -> Instant {
        info!(
            command = command_name,
            args = args,
            "Command started"
        );
        Instant::now()
    }

    pub fn log_command_success(command_name: &str, start_time: Instant) {
        let duration = start_time.elapsed();
        info!(
            command = command_name,
            duration_ms = duration.as_millis(),
            "Command completed successfully"
        );
    }

    pub fn log_command_error(command_name: &str, error: &str, start_time: Instant) {
        let duration = start_time.elapsed();
        error!(
            command = command_name,
            error = error,
            duration_ms = duration.as_millis(),
            "Command failed"
        );
    }

    pub fn log_api_request(endpoint: &str, method: &str, params: &str) {
        debug!(
            endpoint = endpoint,
            method = method,
            params = params,
            "API request"
        );
    }

    pub fn log_api_response(endpoint: &str, status: &str, duration_ms: u64) {
        debug!(
            endpoint = endpoint,
            status = status,
            duration_ms = duration_ms,
            "API response"
        );
    }

    pub fn log_rate_limit_warning(endpoint: &str, remaining: u32) {
        warn!(
            endpoint = endpoint,
            remaining = remaining,
            "Approaching rate limit"
        );
    }
}

// Macro for easy command logging
#[macro_export]
macro_rules! log_command {
    ($name:expr, $body:expr) => {{
        let start = $crate::middleware::logging::CommandLogger::log_command_start($name, "");
        match $body {
            Ok(result) => {
                $crate::middleware::logging::CommandLogger::log_command_success($name, start);
                Ok(result)
            }
            Err(e) => {
                $crate::middleware::logging::CommandLogger::log_command_error(
                    $name,
                    &e.to_string(),
                    start,
                );
                Err(e)
            }
        }
    }};
}