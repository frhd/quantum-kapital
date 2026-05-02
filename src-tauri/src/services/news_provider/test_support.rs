//! Mock-friendly [`NewsProvider`] for downstream tests.
//!
//! Tests for the MCP `get_news` tool, the `tracker_get_news` Tauri
//! command, the `TrackerRunner`, and any future caller of the trait
//! can pre-load symbols via [`FakeNewsProvider::insert`] or program a
//! single error response via [`FakeNewsProvider::fail_with`]. Mirrors
//! the [`super::super::fundamentals_provider::test_support`] layout so
//! test fixtures stay co-located with the trait they fake.

#![allow(dead_code)] // used by tests in other modules; not all helpers are used everywhere.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::ibkr::types::news::NewsItem;

use super::{NewsError, NewsProvider};

/// Programmable in-memory [`NewsProvider`]. Default state has no rows
/// and no error — every `fetch` returns `Ok(Vec::new())` (the canonical
/// "no news for symbol" signal — see the trait docs) until a test
/// calls [`Self::insert`] or [`Self::fail_with`].
pub struct FakeNewsProvider {
    rows: Mutex<HashMap<String, Vec<NewsItem>>>,
    /// Error message to surface on every `fetch`. When `Some`, takes
    /// precedence over `rows` so error-path tests are deterministic.
    forced_error: Mutex<Option<String>>,
}

impl FakeNewsProvider {
    pub fn new() -> Self {
        Self {
            rows: Mutex::new(HashMap::new()),
            forced_error: Mutex::new(None),
        }
    }

    /// Pre-load `items` for `symbol`. Symbol is uppercased so callers
    /// can pass either case.
    pub fn insert(&self, symbol: impl Into<String>, items: Vec<NewsItem>) {
        let key = symbol.into().to_uppercase();
        self.rows
            .lock()
            .expect("FakeNewsProvider rows mutex poisoned")
            .insert(key, items);
    }

    /// Force every subsequent `fetch` to return [`NewsError::Other`]
    /// with `message`. Single-shot is intentionally NOT supported —
    /// tests that need different errors on successive calls should
    /// construct two providers.
    pub fn fail_with(&self, message: impl Into<String>) {
        *self
            .forced_error
            .lock()
            .expect("FakeNewsProvider forced_error mutex poisoned") = Some(message.into());
    }
}

impl Default for FakeNewsProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NewsProvider for FakeNewsProvider {
    async fn fetch(&self, symbol: &str, _lookback_hours: u32) -> Result<Vec<NewsItem>, NewsError> {
        if let Some(msg) = self
            .forced_error
            .lock()
            .expect("FakeNewsProvider forced_error mutex poisoned")
            .clone()
        {
            return Err(NewsError::Other(msg));
        }
        let key = symbol.trim().to_uppercase();
        let rows = self
            .rows
            .lock()
            .expect("FakeNewsProvider rows mutex poisoned");
        Ok(rows.get(&key).cloned().unwrap_or_default())
    }
}
