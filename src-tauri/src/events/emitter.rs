use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock;

use crate::ibkr::types::tracker::{Setup, TrackerStatus};
use crate::ibkr::types::{DataTier, ScannerData};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AppEvent {
    // Connection events
    ConnectionStatusChanged {
        connected: bool,
        message: String,
    },
    ConnectionError {
        error: String,
    },

    // Account events
    AccountUpdate {
        account_id: String,
        data: serde_json::Value,
    },
    AccountsListChanged {
        accounts: Vec<String>,
    },
    DailyPnLUpdate {
        account: String,
        daily_pnl: f64,
        unrealized_pnl: Option<f64>,
        realized_pnl: Option<f64>,
    },

    // Market data events
    MarketDataUpdate {
        symbol: String,
        data: serde_json::Value,
    },
    MarketDataSubscribed {
        symbol: String,
    },
    MarketDataUnsubscribed {
        symbol: String,
    },
    /// Emitted by `IbkrClient` after the connect-time probe finishes
    /// (or on disconnect, where it carries `DataTier::Unknown`). Lets
    /// the UI banner and any tier-gated consumer react without polling
    /// `IbkrState`.
    DataTierDetected {
        tier: DataTier,
    },

    // Order events
    OrderPlaced {
        order_id: i32,
        symbol: String,
    },
    OrderFilled {
        order_id: i32,
        filled_qty: f64,
    },
    OrderCancelled {
        order_id: i32,
    },
    OrderError {
        order_id: Option<i32>,
        error: String,
    },

    // Position events
    PositionUpdate {
        symbol: String,
        position: f64,
    },
    PositionsRefreshed,

    // Scanner events
    ScannerUpdate {
        results: Vec<ScannerData>,
    },

    // Tracker / scheduling events
    /// Emitted by `TrackerRunner` after a detector hit is persisted.
    /// The `thesis` field stays `None` until Phase 17 wires the LLM
    /// thesis prompt; the frontend treats absence as "no narrative
    /// yet".
    SetupDetected {
        setup: Box<Setup>,
        thesis: Option<String>,
    },
    /// Emitted by `TrackerStateMachine::mark_invalidated` when a
    /// persisted setup is flipped to `Invalidated`. Carries the
    /// reason so the UI can surface it in a toast.
    SetupInvalidated {
        setup_id: i64,
        symbol: String,
        reason: String,
    },
    /// Emitted whenever a tracker row's status changes. Lets the
    /// frontend update the watchlist row badge without a full
    /// re-fetch.
    TickerStatusChanged {
        symbol: String,
        from: TrackerStatus,
        to: TrackerStatus,
    },
    /// Emitted by the EOD scheduler after a successful 16:05 ET sweep.
    /// `date` is the ET trading-day date. `ranked_count` is `0` until
    /// Phase 20's daily ranker fills it in.
    MorningPackReady {
        date: NaiveDate,
        ranked_count: usize,
    },

    // Phase 02 — research artifacts written via MCP write tools.
    /// Emitted after `write_research_note` persists a row. Carries the
    /// minimal identifiers the UI needs to refresh its query for the
    /// affected symbol / alert without a full refetch of every note.
    ResearchNoteWritten {
        note_id: i64,
        symbol: String,
        alert_id: Option<i64>,
        setup_id: Option<i64>,
    },
    /// Emitted after `write_morning_pack` upserts an agent-authored
    /// pack. The frontend re-queries the pack for `date` to render the
    /// new ranked ideas.
    AgentMorningPackWritten {
        date: NaiveDate,
        idea_count: usize,
    },
    /// Emitted after `ack_alert` records a decision. The UI flips the
    /// alert's row treatment (e.g. greying acted/passed alerts) without
    /// reloading the whole feed.
    AlertDecisionRecorded {
        alert_id: i64,
        decision: String,
        note_id: Option<i64>,
    },
    /// Phase 6 — emitted after `mark_alert_enriched` flips an alert
    /// from "pending dive" to "enriched" (or "dive skipped"). The UI
    /// flips the alert detail panel from "Enriching..." to "Deep dive
    /// ready" / "skipped" using `research_note_id`.
    AlertEnriched {
        alert_id: i64,
        research_note_id: Option<i64>,
    },
    /// Phase 6 — emitted when the per-alert dive bypassed an alert
    /// (currently: global LLM budget below the per-alert reserve). Lets
    /// the UI render a "deep dive skipped (budget)" badge without
    /// having to inspect the audit row.
    AlertDiveSkipped {
        alert_id: i64,
        reason: String,
    },
    /// Phase 4 (AV strip-out) — emitted after the MCP `set_fundamentals`
    /// tool persists a manual row. The analysis UI re-queries
    /// `get_fundamentals(symbol)` and re-renders the projection so the
    /// freshly pasted snapshot is immediately reflected without polling.
    FundamentalsManualWritten {
        symbol: String,
        as_of_date: String,
        source: String,
    },

    // System events
    RateLimitWarning {
        remaining: u32,
    },
    SystemError {
        error: String,
    },
}

impl AppEvent {
    fn name(&self) -> &'static str {
        match self {
            AppEvent::ConnectionStatusChanged { .. } => "connection-status-changed",
            AppEvent::ConnectionError { .. } => "connection-error",
            AppEvent::AccountUpdate { .. } => "account-update",
            AppEvent::AccountsListChanged { .. } => "accounts-list-changed",
            AppEvent::DailyPnLUpdate { .. } => "daily-pnl-update",
            AppEvent::MarketDataUpdate { .. } => "market-data-update",
            AppEvent::MarketDataSubscribed { .. } => "market-data-subscribed",
            AppEvent::MarketDataUnsubscribed { .. } => "market-data-unsubscribed",
            AppEvent::DataTierDetected { .. } => "data-tier-detected",
            AppEvent::OrderPlaced { .. } => "order-placed",
            AppEvent::OrderFilled { .. } => "order-filled",
            AppEvent::OrderCancelled { .. } => "order-cancelled",
            AppEvent::OrderError { .. } => "order-error",
            AppEvent::PositionUpdate { .. } => "position-update",
            AppEvent::PositionsRefreshed => "positions-refreshed",
            AppEvent::ScannerUpdate { .. } => "scanner-update",
            AppEvent::SetupDetected { .. } => "setup-detected",
            AppEvent::SetupInvalidated { .. } => "setup-invalidated",
            AppEvent::TickerStatusChanged { .. } => "ticker-status-changed",
            AppEvent::MorningPackReady { .. } => "morning-pack-ready",
            AppEvent::ResearchNoteWritten { .. } => "research-note-written",
            AppEvent::AgentMorningPackWritten { .. } => "agent-morning-pack-written",
            AppEvent::AlertDecisionRecorded { .. } => "alert-decision-recorded",
            AppEvent::AlertEnriched { .. } => "alert-enriched",
            AppEvent::AlertDiveSkipped { .. } => "alert-dive-skipped",
            AppEvent::FundamentalsManualWritten { .. } => "fundamentals-manual-written",
            AppEvent::RateLimitWarning { .. } => "rate-limit-warning",
            AppEvent::SystemError { .. } => "system-error",
        }
    }
}

pub struct EventEmitter {
    app_handle: Arc<RwLock<Option<AppHandle>>>,
    /// Test seam: when `Some`, every `emit` call records the event
    /// here in addition to (or instead of) forwarding to Tauri. Lets
    /// tests assert on emissions without standing up a full Tauri
    /// runtime. Production code never enables this.
    capture: Arc<RwLock<Option<Vec<AppEvent>>>>,
}

#[allow(dead_code)]
impl EventEmitter {
    pub fn new() -> Self {
        Self {
            app_handle: Arc::new(RwLock::new(None)),
            capture: Arc::new(RwLock::new(None)),
        }
    }

    /// Test-only: returns an emitter with capture pre-enabled. The
    /// returned emitter still works fine if an `app_handle` is later
    /// attached, but tests typically just call `captured()`.
    pub fn for_capture() -> Self {
        Self {
            app_handle: Arc::new(RwLock::new(None)),
            capture: Arc::new(RwLock::new(Some(Vec::new()))),
        }
    }

    pub async fn enable_capture(&self) {
        let mut cap = self.capture.write().await;
        if cap.is_none() {
            *cap = Some(Vec::new());
        }
    }

    pub async fn captured(&self) -> Vec<AppEvent> {
        self.capture
            .read()
            .await
            .as_ref()
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    pub async fn set_app_handle(&self, handle: AppHandle) {
        let mut app_handle = self.app_handle.write().await;
        *app_handle = Some(handle);
    }

    pub async fn emit(&self, event: AppEvent) -> Result<(), String> {
        // Record into the capture buffer first (if enabled). We clone
        // the event so the dispatch path below still owns its copy.
        let captured = {
            let mut cap = self.capture.write().await;
            if let Some(buf) = cap.as_mut() {
                buf.push(event.clone());
                true
            } else {
                false
            }
        };

        let app_handle = self.app_handle.read().await;
        if let Some(handle) = app_handle.as_ref() {
            handle
                .emit(event.name(), &event)
                .map_err(|e| format!("Failed to emit event: {e}"))?;
            return Ok(());
        }

        if captured {
            // Tests don't attach an app handle; capture is the sink.
            Ok(())
        } else {
            Err("App handle not initialized".to_string())
        }
    }

    pub async fn emit_to_window(&self, window: &str, event: AppEvent) -> Result<(), String> {
        let app_handle = self.app_handle.read().await;

        if let Some(handle) = app_handle.as_ref() {
            if let Some(window) = handle.get_webview_window(window) {
                window
                    .emit(event.name(), &event)
                    .map_err(|e| format!("Failed to emit event to window: {e}"))?;

                Ok(())
            } else {
                Err(format!("Window '{window}' not found"))
            }
        } else {
            Err("App handle not initialized".to_string())
        }
    }
}
