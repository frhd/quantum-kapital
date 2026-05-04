use crate::ibkr::commands::trading::parse_date_arg;
use crate::ibkr::error::IbkrError;
use crate::ibkr::mocks::{test_fixtures, IbkrClientTrait, MockIbkrClient};
use crate::ibkr::types::*;
use chrono::{NaiveDate, TimeZone, Utc};
use chrono_tz::America::New_York;

// Note: These tests demonstrate how to test Tauri commands.
// In a real implementation, you would need to refactor the commands
// to accept the client trait as a parameter for testability.

#[tokio::test]
async fn test_command_connect_flow() {
    // This test demonstrates the expected flow for the connect command
    let client = MockIbkrClient::new();

    // Simulate the connect command flow
    let config = ConnectionConfig::default();
    assert_eq!(config.host, "127.0.0.1");
    assert_eq!(config.port, 4004);
    assert_eq!(config.client_id, 100);

    // Connect should succeed with default config
    let result = client.connect().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_command_get_positions_flow() {
    // This test demonstrates the expected flow for get_positions command
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    // Set up test data
    let positions = vec![
        test_fixtures::sample_position(),
        Position {
            symbol: "MSFT".to_string(),
            position: 50.0,
            market_price: 300.0,
            market_value: 15000.0,
            average_cost: 290.0,
            unrealized_pnl: 500.0,
            realized_pnl: 0.0,
            account: "DU123456".to_string(),
            contract_type: "STK".to_string(),
            currency: "USD".to_string(),
            exchange: "NASDAQ".to_string(),
            local_symbol: "MSFT".to_string(),
            ..Default::default()
        },
    ];
    client.set_positions(positions).await;

    // Get positions
    let result = client.get_positions("DU123456").await.unwrap();
    assert_eq!(result.len(), 2);

    // Verify position details
    let aapl = result.iter().find(|p| p.symbol == "AAPL").unwrap();
    assert_eq!(aapl.position, 100.0);
    assert_eq!(aapl.unrealized_pnl, 500.0);
}

#[tokio::test]
async fn test_command_account_summary_parsing() {
    // This test demonstrates how account summary data should be parsed
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let summary_items = vec![
        AccountSummary {
            account: "DU123456".to_string(),
            tag: "NetLiquidation".to_string(),
            value: "100000.0".to_string(),
            currency: "USD".to_string(),
        },
        AccountSummary {
            account: "DU123456".to_string(),
            tag: "UnrealizedPnL".to_string(),
            value: "2500.0".to_string(),
            currency: "USD".to_string(),
        },
        AccountSummary {
            account: "DU123456".to_string(),
            tag: "RealizedPnL".to_string(),
            value: "1500.0".to_string(),
            currency: "USD".to_string(),
        },
    ];
    client.set_account_summary(summary_items).await;

    let result = client.get_account_summary("DU123456").await.unwrap();
    assert_eq!(result.len(), 3);

    // In a real command, you would parse these into a structured format
    let net_liq_value = result
        .iter()
        .find(|s| s.tag == "NetLiquidation")
        .map(|s| s.value.parse::<f64>().unwrap())
        .unwrap();
    assert_eq!(net_liq_value, 100000.0);
}

#[tokio::test]
async fn test_command_order_validation() {
    // This test demonstrates order validation logic
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    // Test valid order
    let valid_order = OrderRequest {
        symbol: "AAPL".to_string(),
        action: OrderAction::Buy,
        quantity: 100.0,
        order_type: OrderType::Limit,
        price: Some(150.0),
    };

    let result = client.place_order(valid_order).await;
    assert!(result.is_ok());

    // Test market order (no price needed)
    let market_order = OrderRequest {
        symbol: "AAPL".to_string(),
        action: OrderAction::Sell,
        quantity: 50.0,
        order_type: OrderType::Market,
        price: None,
    };

    let result = client.place_order(market_order).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_command_error_handling() {
    // This test demonstrates proper error handling in commands
    let client = MockIbkrClient::new();

    // Test operation when not connected
    let result = client.get_positions("DU123456").await;
    assert!(matches!(result, Err(IbkrError::NotConnected)));

    // Test with connection error
    let error_client = MockIbkrClient::with_error(IbkrError::ConnectionFailed(
        "Gateway not running".to_string(),
    ));
    let result = error_client.connect().await;
    assert!(matches!(result, Err(IbkrError::ConnectionFailed(_))));
}

#[tokio::test]
async fn test_command_concurrent_operations() {
    // This test demonstrates handling concurrent operations
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    // Set up test data
    client
        .set_positions(vec![test_fixtures::sample_position()])
        .await;
    client
        .set_account_summary(test_fixtures::sample_account_summary())
        .await;

    // Simulate concurrent requests
    let positions_future = client.get_positions("DU123456");
    let summary_future = client.get_account_summary("DU123456");
    let accounts_future = client.get_accounts();

    let (positions, summary, accounts) =
        tokio::join!(positions_future, summary_future, accounts_future);

    assert!(positions.is_ok());
    assert!(summary.is_ok());
    assert!(accounts.is_ok());

    assert_eq!(positions.unwrap().len(), 1);
    assert!(!summary.unwrap().is_empty());
    assert_eq!(accounts.unwrap().len(), 1);
}

// ---- ibkr_get_executions / Phase 24 ----

fn et_datetime(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> chrono::DateTime<Utc> {
    let naive = NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_hms_opt(hour, minute, 0)
        .unwrap();
    New_York
        .from_local_datetime(&naive)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

fn sample_execution(
    symbol: &str,
    side: ExecutionSide,
    qty: f64,
    avg_price: f64,
    exec_time: chrono::DateTime<Utc>,
    exec_id: &str,
) -> IbkrExecution {
    IbkrExecution {
        symbol: symbol.to_string(),
        side,
        qty,
        avg_price,
        exec_time,
        order_id: 1001,
        exec_id: exec_id.to_string(),
        account: "DU123456".to_string(),
        contract_type: "STK".to_string(),
        expiry: None,
        strike: None,
        right: None,
        multiplier: None,
        commission: None,
        realized_pnl: None,
        currency: Some("USD".to_string()),
        commission_currency: None,
    }
}

#[tokio::test]
async fn executions_filters_to_requested_date() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let target_date = NaiveDate::from_ymd_opt(2026, 4, 29).unwrap();
    client
        .set_executions(vec![
            sample_execution(
                "AAPL",
                ExecutionSide::Bought,
                100.0,
                150.25,
                et_datetime(2026, 4, 29, 10, 30),
                "0001",
            ),
            sample_execution(
                "MSFT",
                ExecutionSide::Sold,
                50.0,
                420.0,
                et_datetime(2026, 4, 28, 15, 45),
                "0002",
            ),
            sample_execution(
                "TSLA",
                ExecutionSide::Bought,
                25.0,
                275.5,
                et_datetime(2026, 4, 30, 9, 35),
                "0003",
            ),
        ])
        .await;

    let result = client.executions(target_date).await.unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].symbol, "AAPL");
    assert_eq!(result[0].exec_id, "0001");
}

#[tokio::test]
async fn executions_serializes_for_frontend() {
    let exec = sample_execution(
        "AAPL",
        ExecutionSide::Bought,
        100.0,
        150.25,
        et_datetime(2026, 4, 29, 10, 30),
        "0001",
    );

    let json = serde_json::to_value(&exec).unwrap();

    // Field names must be snake_case to match the rest of the IBKR types.
    assert!(json.get("symbol").is_some());
    assert!(json.get("side").is_some());
    assert!(json.get("qty").is_some());
    assert!(json.get("avg_price").is_some());
    assert!(json.get("exec_time").is_some());
    assert!(json.get("order_id").is_some());
    assert!(json.get("exec_id").is_some());

    // ExecutionSide serializes as lowercase "bought" / "sold".
    assert_eq!(json.get("side").unwrap().as_str().unwrap(), "bought");

    // Round-trip preserves all fields.
    let round_tripped: IbkrExecution = serde_json::from_value(json).unwrap();
    assert_eq!(round_tripped.symbol, exec.symbol);
    assert_eq!(round_tripped.qty, exec.qty);
    assert_eq!(round_tripped.avg_price, exec.avg_price);
    assert_eq!(round_tripped.order_id, exec.order_id);
    assert_eq!(round_tripped.exec_id, exec.exec_id);
}

#[tokio::test]
async fn executions_empty_when_no_fills() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let date = NaiveDate::from_ymd_opt(2026, 4, 29).unwrap();
    let result = client.executions(date).await.unwrap();

    assert!(result.is_empty());
}

#[tokio::test]
async fn executions_disconnected_returns_not_connected() {
    let client = MockIbkrClient::new();

    let date = NaiveDate::from_ymd_opt(2026, 4, 29).unwrap();
    let result = client.executions(date).await;

    assert!(matches!(result, Err(IbkrError::NotConnected)));
}

#[test]
fn command_parses_correct_date() {
    let parsed = parse_date_arg("2026-04-29").unwrap();
    assert_eq!(parsed, NaiveDate::from_ymd_opt(2026, 4, 29).unwrap());
}

#[test]
fn command_rejects_malformed_date() {
    // Wrong separator
    let err = parse_date_arg("2026/04/29").unwrap_err();
    assert!(err.contains("invalid date"));

    // Out-of-range month
    let err = parse_date_arg("2026-13-01").unwrap_err();
    assert!(err.contains("invalid date"));

    // Empty string
    let err = parse_date_arg("").unwrap_err();
    assert!(err.contains("invalid date"));
}
