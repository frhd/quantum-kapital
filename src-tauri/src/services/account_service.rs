use crate::events::AppEvent;
use crate::ibkr::state::IbkrState;
use crate::ibkr::types::{AccountSummary, Position};
use crate::utils;
use std::sync::Arc;

#[allow(dead_code)]
pub struct AccountService {
    state: Arc<IbkrState>,
}

#[allow(dead_code)]
impl AccountService {
    pub fn new(state: Arc<IbkrState>) -> Self {
        Self { state }
    }

    /// Get all accounts with caching and event emission
    pub async fn get_accounts_with_retry(&self, max_retries: u32) -> Result<Vec<String>, String> {
        let mut retries = 0;
        let mut last_error = String::new();

        while retries < max_retries {
            match self.state.client.get_accounts().await {
                Ok(accounts) => {
                    // Emit event for successful account list retrieval
                    let _ = self.state.event_emitter.emit(AppEvent::AccountsListChanged {
                        accounts: accounts.clone(),
                    }).await;
                    
                    return Ok(accounts);
                }
                Err(e) => {
                    last_error = e.to_string();
                    retries += 1;
                    
                    if retries < max_retries {
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    }
                }
            }
        }

        Err(format!("Failed after {} retries: {}", max_retries, last_error))
    }

    /// Get account summary with enhanced formatting
    pub async fn get_formatted_account_summary(
        &self,
        account: &str,
    ) -> Result<AccountSummaryReport, String> {
        let summaries = self.state.client.get_account_summary(account).await
            .map_err(|e| e.to_string())?;

        let mut report = AccountSummaryReport {
            account: account.to_string(),
            total_value: 0.0,
            cash_balance: 0.0,
            securities_value: 0.0,
            buying_power: 0.0,
            excess_liquidity: 0.0,
            currency: "USD".to_string(),
            timestamp: utils::current_timestamp_ms(),
            details: vec![],
        };

        for summary in summaries {
            match summary.tag.as_str() {
                "TotalCashValue" => report.cash_balance = summary.value.parse().unwrap_or(0.0),
                "NetLiquidation" => report.total_value = summary.value.parse().unwrap_or(0.0),
                "GrossPositionValue" => report.securities_value = summary.value.parse().unwrap_or(0.0),
                "BuyingPower" => report.buying_power = summary.value.parse().unwrap_or(0.0),
                "ExcessLiquidity" => report.excess_liquidity = summary.value.parse().unwrap_or(0.0),
                _ => {}
            }
            
            report.details.push(summary);
        }

        // Emit account update event
        let _ = self.state.event_emitter.emit(AppEvent::AccountUpdate {
            account_id: account.to_string(),
            data: serde_json::to_value(&report).unwrap_or_default(),
        }).await;

        Ok(report)
    }

    /// Get positions with P&L calculation
    pub async fn get_positions_with_pnl(&self) -> Result<Vec<PositionWithPnL>, String> {
        // Check cache first
        if !self.state.is_cache_stale(30).await {
            let cached = self.state.get_cached_positions().await;
            return Ok(cached.into_iter().map(|p| PositionWithPnL::from_position(p)).collect());
        }

        // Fetch fresh data
        let positions = self.state.client.get_positions().await
            .map_err(|e| e.to_string())?;

        // Cache the positions
        self.state.cache_positions(positions.clone()).await;

        // Calculate P&L for each position
        let positions_with_pnl: Vec<PositionWithPnL> = positions
            .into_iter()
            .map(|p| PositionWithPnL::from_position(p))
            .collect();

        Ok(positions_with_pnl)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountSummaryReport {
    pub account: String,
    pub total_value: f64,
    pub cash_balance: f64,
    pub securities_value: f64,
    pub buying_power: f64,
    pub excess_liquidity: f64,
    pub currency: String,
    pub timestamp: i64,
    pub details: Vec<AccountSummary>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PositionWithPnL {
    pub position: Position,
    pub pnl_percentage: f64,
    pub daily_pnl: Option<f64>,
    pub formatted_pnl: String,
    pub formatted_value: String,
}

#[allow(dead_code)]
impl PositionWithPnL {
    fn from_position(position: Position) -> Self {
        let pnl_percentage = utils::calculate_percentage_change(
            position.average_cost * position.position.abs(),
            position.market_value,
        );

        let formatted_pnl = utils::format_currency(position.unrealized_pnl, &position.currency);
        let formatted_value = utils::format_currency(position.market_value, &position.currency);

        Self {
            position,
            pnl_percentage,
            daily_pnl: None, // Would need historical data to calculate
            formatted_pnl,
            formatted_value,
        }
    }
}