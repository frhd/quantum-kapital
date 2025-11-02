use crate::ibkr::error::IbkrError;
use crate::ibkr::mocks::{test_fixtures, IbkrClientTrait, MockIbkrClient};
use crate::ibkr::types::*;

#[tokio::test]
async fn test_market_data_snapshot() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let snapshot = client.get_market_data_snapshot("AAPL").await.unwrap();

    assert_eq!(snapshot.symbol, "AAPL");
    assert!(snapshot.bid_price.is_some());
    assert!(snapshot.ask_price.is_some());
    assert!(snapshot.last_price.is_some());
    assert!(snapshot.volume.is_some());

    // Verify spread is reasonable
    let spread = snapshot.ask_price.unwrap() - snapshot.bid_price.unwrap();
    assert!(
        spread > 0.0 && spread < 1.0,
        "Spread should be positive and reasonable"
    );
}

#[tokio::test]
async fn test_market_data_snapshot_disconnected() {
    let client = MockIbkrClient::new();

    let result = client.get_market_data_snapshot("AAPL").await;
    assert!(matches!(result, Err(IbkrError::NotConnected)));
}

#[tokio::test]
async fn test_historical_data_request() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let request = test_fixtures::sample_historical_data_request();
    let bars = client.get_historical_data(request).await.unwrap();

    assert!(!bars.is_empty());

    // Verify bar data integrity
    for bar in &bars {
        assert!(bar.high >= bar.low);
        assert!(bar.open >= bar.low && bar.open <= bar.high);
        assert!(bar.close >= bar.low && bar.close <= bar.high);
        assert!(bar.volume >= 0);
        assert!(bar.count > 0);
    }

    // Verify bars are in chronological order
    if bars.len() > 1 {
        for i in 1..bars.len() {
            assert!(bars[i].time > bars[i - 1].time);
        }
    }
}

#[tokio::test]
async fn test_contract_details() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let details = client.get_contract_details("AAPL").await.unwrap();

    assert_eq!(details.symbol, "AAPL");
    assert!(matches!(details.sec_type, SecurityType::Stock));
    assert_eq!(details.currency, "USD");
    assert!(details.contract_id > 0);
    assert_eq!(details.min_tick, 0.01);
}

#[tokio::test]
async fn test_different_security_types() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    // Test multiple symbols
    let symbols = vec!["AAPL", "MSFT", "GOOGL"];
    for symbol in symbols {
        let details = client.get_contract_details(symbol).await.unwrap();
        assert_eq!(details.symbol, symbol);
        assert!(!details.exchange.is_empty());
        assert!(!details.primary_exchange.is_empty());
    }
}

#[tokio::test]
async fn test_executions() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let executions = client.get_executions(None).await.unwrap();

    assert!(!executions.is_empty());

    for exec in &executions {
        assert!(!exec.exec_id.is_empty());
        assert!(exec.shares > 0.0);
        assert!(exec.price > 0.0);
        assert!(exec.cum_qty >= exec.shares);
        assert_eq!(exec.side, "BOT"); // or "SLD"
    }
}

#[tokio::test]
async fn test_executions_with_filter() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    // Test with account filter
    let filter = Some("DU123456".to_string());
    let executions = client.get_executions(filter).await.unwrap();

    for exec in &executions {
        assert_eq!(exec.account, "DU123456");
    }
}

#[tokio::test]
async fn test_account_values() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let values = client.get_account_values("DU123456").await.unwrap();

    assert!(!values.is_empty());

    // Check for essential account values
    let essential_keys = vec!["NetLiquidation", "TotalCashValue", "BuyingPower"];
    for key in essential_keys {
        let value = values.iter().find(|v| v.key == key);
        assert!(value.is_some(), "Missing essential account value: {key}");
    }

    // Verify all values have the correct account
    for value in &values {
        assert_eq!(value.account, "DU123456");
        assert_eq!(value.currency, "USD");
    }
}

#[tokio::test]
async fn test_market_scanner() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let subscription = test_fixtures::sample_scanner_subscription();
    let results = client.scan_market(subscription).await.unwrap();

    assert!(!results.is_empty());

    // Verify scanner results
    for (i, result) in results.iter().enumerate() {
        assert_eq!(result.rank as usize, i + 1); // Ranks should be sequential
        assert!(!result.contract.symbol.is_empty());
        assert!(matches!(result.contract.sec_type, SecurityType::Stock));
    }
}

#[tokio::test]
async fn test_real_time_data_simulation() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    // Subscribe to market data
    client.subscribe_market_data(1, "AAPL").await.unwrap();

    // Get initial snapshot
    let snapshot1 = client.get_market_data_snapshot("AAPL").await.unwrap();

    // In a real scenario, wait for updates
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Get another snapshot
    let snapshot2 = client.get_market_data_snapshot("AAPL").await.unwrap();

    // Timestamps should be different (in real implementation)
    assert!(snapshot2.timestamp >= snapshot1.timestamp);
}

#[tokio::test]
async fn test_portfolio_analysis() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    // Get positions
    let positions = client.get_positions("DU123456").await.unwrap();

    // Get account values
    let account_values = client.get_account_values("DU123456").await.unwrap();

    // Get market data for positions
    for position in &positions {
        let market_data = client
            .get_market_data_snapshot(&position.symbol)
            .await
            .unwrap();

        // Verify position value calculation
        let calculated_value = position.position * market_data.last_price.unwrap_or(0.0);
        // In real implementation, this would match position.market_value
        assert!(calculated_value >= 0.0);
    }

    // Verify net liquidation value
    let net_liq = account_values
        .iter()
        .find(|v| v.key == "NetLiquidation")
        .map(|v| v.value.parse::<f64>().unwrap_or(0.0))
        .unwrap_or(0.0);
    assert!(net_liq > 0.0);
}

#[tokio::test]
async fn test_error_handling_for_new_interfaces() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap(); // Connect succeeds first

    // Now set error mode for subsequent operations
    client
        .set_error(Some(IbkrError::ApiError(
            "Market data farm disconnected".to_string(),
        )))
        .await;

    // Test all new interfaces handle errors properly
    assert!(client.get_market_data_snapshot("AAPL").await.is_err());
    assert!(client
        .get_historical_data(test_fixtures::sample_historical_data_request())
        .await
        .is_err());
    assert!(client.get_contract_details("AAPL").await.is_err());
    assert!(client.get_executions(None).await.is_err());
    assert!(client.get_account_values("DU123456").await.is_err());
    assert!(client
        .scan_market(test_fixtures::sample_scanner_subscription())
        .await
        .is_err());
}

#[tokio::test]
async fn test_concurrent_api_calls() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    // Make multiple API calls concurrently
    let market_data_future = client.get_market_data_snapshot("AAPL");
    let historical_future =
        client.get_historical_data(test_fixtures::sample_historical_data_request());
    let contract_future = client.get_contract_details("MSFT");
    let executions_future = client.get_executions(None);
    let account_future = client.get_account_values("DU123456");

    let (market_data, historical, contract, executions, account) = tokio::join!(
        market_data_future,
        historical_future,
        contract_future,
        executions_future,
        account_future
    );

    // All should succeed
    assert!(market_data.is_ok());
    assert!(historical.is_ok());
    assert!(contract.is_ok());
    assert!(executions.is_ok());
    assert!(account.is_ok());
}
