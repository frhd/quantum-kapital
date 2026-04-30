use chrono::{DateTime, TimeZone, Utc};

/// Convert a UNIX timestamp (seconds) to UTC. Falls back to `Utc::now()`
/// when the value is out-of-range, since SQLite-backed callers prefer a
/// best-effort decode over a panic on corrupted rows.
pub fn unix_to_utc(ts: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now)
}
