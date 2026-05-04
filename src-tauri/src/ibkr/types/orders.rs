use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionSide {
    Bought,
    Sold,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbkrExecution {
    pub symbol: String,
    pub side: ExecutionSide,
    pub qty: f64,
    pub avg_price: f64,
    pub exec_time: DateTime<Utc>,
    pub order_id: i32,
    pub exec_id: String,
    /// IBKR account number that booked the fill. Carried through from
    /// `ExecutionData.execution.account_number` so multi-account users
    /// can attribute fills correctly.
    #[serde(default)]
    pub account: String,
    /// IBKR `secType` code: `STK`, `OPT`, `FUT`, etc. Mirrors the
    /// `Position.contract_type` field used by `get_positions`.
    #[serde(default)]
    pub contract_type: String,
    /// Option expiry as a `NaiveDate`, parsed from the SDK's
    /// `last_trade_date_or_contract_month`. `None` for non-options or
    /// when the SDK only reports `YYYYMM` (no day).
    #[serde(default)]
    pub expiry: Option<NaiveDate>,
    /// Option strike price; `None` for non-options.
    #[serde(default)]
    pub strike: Option<f64>,
    /// Option right normalised to `"C"` or `"P"`; `None` for non-options.
    #[serde(default)]
    pub right: Option<String>,
    /// Option contract multiplier (typically `"100"` for equity options);
    /// `None` for non-options.
    #[serde(default)]
    pub multiplier: Option<String>,
    /// Commission charged for this fill, as reported by IBKR's
    /// `CommissionReport`. `None` ↔ "not (yet) reported"; a literal `0.0`
    /// is real (free trade).
    #[serde(default)]
    pub commission: Option<f64>,
    /// Realized P&L for the fill (closing legs only), as reported by
    /// IBKR. `None` for opening legs or when the report has not arrived.
    #[serde(default)]
    pub realized_pnl: Option<f64>,
    /// The contract's currency (e.g. `"USD"`).
    #[serde(default)]
    pub currency: Option<String>,
    /// The currency the commission is reported in. May differ from
    /// `currency` for some non-US instruments; identical to it for US
    /// equities and options in practice.
    #[serde(default)]
    pub commission_currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub symbol: String,
    pub action: OrderAction,
    pub quantity: f64,
    pub order_type: OrderType,
    pub price: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderAction {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
    Stop,
    StopLimit,
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub exec_id: String,
    pub time: String,
    pub account: String,
    pub exchange: String,
    pub side: String,
    pub shares: f64,
    pub price: f64,
    pub perm_id: i32,
    pub client_id: i32,
    pub order_id: i32,
    pub liquidation: bool,
    pub cum_qty: f64,
    pub avg_price: f64,
}
