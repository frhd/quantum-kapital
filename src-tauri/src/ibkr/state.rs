use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::client::{IbkrClient, StreamHandle};
use crate::ibkr::types::{ConnectionConfig, ScannerSubscription};
use crate::services::auto_scanner::AutoScannerScheduler;
use crate::services::eod_scheduler::EodScheduler;
use crate::services::intraday_scheduler::IntradayScheduler;
use crate::services::tracker_service::TrackerService;
use crate::services::tracker_state_machine::TrackerStateMachine;
use crate::storage::Db;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct IbkrState {
    pub client: Arc<IbkrClient>,
    pub event_emitter: Arc<EventEmitter>,
    pub config: Arc<RwLock<ConnectionConfig>>,
    pub daily_pnl_handle: Arc<RwLock<Option<StreamHandle>>>,
    pub scanner_handle: Arc<RwLock<Option<StreamHandle>>>,
    pub eod_handle: Arc<RwLock<Option<StreamHandle>>>,
    pub intraday_handle: Arc<RwLock<Option<StreamHandle>>>,
    pub auto_scanner_handle: Arc<RwLock<Option<StreamHandle>>>,
    pub tracker: Arc<TrackerService>,
    pub state_machine: Arc<TrackerStateMachine>,
}

impl IbkrState {
    pub fn new(config: ConnectionConfig, db: Arc<Db>) -> Self {
        let config_arc = Arc::new(RwLock::new(config));
        let event_emitter = Arc::new(EventEmitter::new());
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
        let state_machine = Arc::new(TrackerStateMachine::new(
            Arc::clone(&db),
            Arc::clone(&tracker),
            Arc::clone(&event_emitter),
        ));
        Self {
            client: Arc::new(IbkrClient::with_shared_config(Arc::clone(&config_arc))),
            event_emitter,
            config: config_arc,
            daily_pnl_handle: Arc::new(RwLock::new(None)),
            scanner_handle: Arc::new(RwLock::new(None)),
            eod_handle: Arc::new(RwLock::new(None)),
            intraday_handle: Arc::new(RwLock::new(None)),
            auto_scanner_handle: Arc::new(RwLock::new(None)),
            tracker,
            state_machine,
        }
    }

    pub async fn start_daily_pnl(&self, account: &str) -> Result<(), String> {
        // Replace any existing subscription
        self.stop_daily_pnl().await;

        let handle = self
            .client
            .start_daily_pnl_stream(account, Arc::clone(&self.event_emitter))
            .await
            .map_err(|e| e.to_string())?;
        *self.daily_pnl_handle.write().await = Some(handle);
        Ok(())
    }

    pub async fn stop_daily_pnl(&self) {
        let handle = self.daily_pnl_handle.write().await.take();
        if let Some(handle) = handle {
            handle.stop().await;
        }
    }

    pub async fn start_scanner(&self, opts: ScannerSubscription) -> Result<(), String> {
        // Replace any existing subscription
        self.stop_scanner().await;

        let handle = self
            .client
            .start_scanner_stream(opts, Arc::clone(&self.event_emitter))
            .await
            .map_err(|e| e.to_string())?;
        *self.scanner_handle.write().await = Some(handle);
        Ok(())
    }

    pub async fn stop_scanner(&self) {
        let handle = self.scanner_handle.write().await.take();
        if let Some(handle) = handle {
            handle.stop().await;
        }
    }

    #[allow(dead_code)] // scheduler API surface — UI wiring on roadmap
    pub async fn start_eod_scheduler(&self, scheduler: Arc<EodScheduler>) -> Result<(), String> {
        // Replace any existing scheduler — same pattern as start_scanner.
        self.stop_eod_scheduler().await;
        let handle = scheduler.spawn();
        *self.eod_handle.write().await = Some(handle);
        Ok(())
    }

    #[allow(dead_code)] // scheduler API surface — UI wiring on roadmap
    pub async fn stop_eod_scheduler(&self) {
        let handle = self.eod_handle.write().await.take();
        if let Some(handle) = handle {
            handle.stop().await;
        }
    }

    #[allow(dead_code)] // scheduler API surface — UI wiring on roadmap
    pub async fn start_intraday_scheduler(
        &self,
        scheduler: Arc<IntradayScheduler>,
    ) -> Result<(), String> {
        // Replace any existing scheduler — same pattern as start_scanner.
        self.stop_intraday_scheduler().await;
        let handle = scheduler.spawn();
        *self.intraday_handle.write().await = Some(handle);
        Ok(())
    }

    #[allow(dead_code)] // scheduler API surface — UI wiring on roadmap
    pub async fn stop_intraday_scheduler(&self) {
        let handle = self.intraday_handle.write().await.take();
        if let Some(handle) = handle {
            handle.stop().await;
        }
    }

    pub async fn start_auto_scanner(
        &self,
        scheduler: Arc<AutoScannerScheduler>,
    ) -> Result<(), String> {
        // Replace any running loop — same idempotent pattern as the
        // other scheduler handles.
        self.stop_auto_scanner().await;
        let handle = scheduler.spawn();
        *self.auto_scanner_handle.write().await = Some(handle);
        Ok(())
    }

    pub async fn stop_auto_scanner(&self) {
        let handle = self.auto_scanner_handle.write().await.take();
        if let Some(handle) = handle {
            handle.stop().await;
        }
    }

    pub async fn update_connection_status(&self, connected: bool) {
        let _ = self
            .event_emitter
            .emit(AppEvent::ConnectionStatusChanged {
                connected,
                message: if connected {
                    "Connected to IBKR".to_string()
                } else {
                    "Disconnected from IBKR".to_string()
                },
            })
            .await;
    }

    pub async fn increment_client_id(&self) {
        let mut config = self.config.write().await;
        config.client_id += 1;
        tracing::info!("🔵 CLIENT ID INCREMENTED TO: {}", config.client_id);
    }
}
