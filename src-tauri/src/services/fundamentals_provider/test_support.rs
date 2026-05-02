//! Mock-friendly [`FundamentalsProvider`] for downstream tests.
//!
//! Tests for the MCP `get_fundamentals` tool, the analysis Tauri
//! commands, and any future caller of the trait can pre-load symbols
//! via [`FakeFundamentalsProvider::insert`] or program a single error
//! response via [`FakeFundamentalsProvider::fail_with`]. Mirrors the
//! `mcp/tools/test_support.rs` layout so test fixtures stay co-located
//! with the trait they fake.

#![allow(dead_code)] // used by tests in other modules; not all helpers are used everywhere.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::ibkr::types::FundamentalData;

use super::{FundamentalsError, FundamentalsProvider};

/// Programmable in-memory [`FundamentalsProvider`]. Default state has
/// no rows and no error — every `fetch` returns
/// [`FundamentalsError::NotFound`] until a test calls [`Self::insert`]
/// or [`Self::fail_with`].
pub struct FakeFundamentalsProvider {
    rows: Mutex<HashMap<String, FundamentalData>>,
    /// Error message to surface on every `fetch`. When `Some`, takes
    /// precedence over `rows` so error-path tests are deterministic.
    forced_error: Mutex<Option<String>>,
}

impl FakeFundamentalsProvider {
    pub fn new() -> Self {
        Self {
            rows: Mutex::new(HashMap::new()),
            forced_error: Mutex::new(None),
        }
    }

    /// Pre-load `data` for `symbol`. Symbol is uppercased so callers can
    /// pass either case.
    pub fn insert(&self, symbol: impl Into<String>, data: FundamentalData) {
        let key = symbol.into().to_uppercase();
        self.rows
            .lock()
            .expect("FakeFundamentalsProvider rows mutex poisoned")
            .insert(key, data);
    }

    /// Force every subsequent `fetch` to return
    /// [`FundamentalsError::Other`] with `message`. Single-shot is
    /// intentionally NOT supported — tests that need different errors on
    /// successive calls should construct two providers.
    pub fn fail_with(&self, message: impl Into<String>) {
        *self
            .forced_error
            .lock()
            .expect("FakeFundamentalsProvider forced_error mutex poisoned") = Some(message.into());
    }
}

impl Default for FakeFundamentalsProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FundamentalsProvider for FakeFundamentalsProvider {
    async fn fetch(&self, symbol: &str) -> Result<FundamentalData, FundamentalsError> {
        if let Some(msg) = self
            .forced_error
            .lock()
            .expect("FakeFundamentalsProvider forced_error mutex poisoned")
            .clone()
        {
            return Err(FundamentalsError::Other(msg));
        }
        let key = symbol.trim().to_uppercase();
        let rows = self
            .rows
            .lock()
            .expect("FakeFundamentalsProvider rows mutex poisoned");
        match rows.get(&key).cloned() {
            Some(data) => Ok(data),
            None => Err(FundamentalsError::NotFound(key)),
        }
    }
}
