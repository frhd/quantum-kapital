use crate::ibkr::mocks::{MockIbkrClient, IbkrClientTrait, test_fixtures};
use crate::ibkr::types::*;

#[tokio::test]
async fn test_full_trading_session_flow() {
    // This integration test simulates a complete trading session
    let client = MockIbkrClient::new();
    
    // Step 1: Connect to IBKR
    let connect_result = client.connect().await;
    assert!(connect_result.is_ok(), "Failed to connect to IBKR");
    assert!(client.is_connected().await);
    
    // Step 2: Get available accounts
    let accounts = client.get_accounts().await.unwrap();
    assert!(!accounts.is_empty(), "No accounts available");
    let account = &accounts[0];
    
    // Step 3: Get account summary
    client.set_account_summary(test_fixtures::sample_account_summary()).await;
    let summary = client.get_account_summary(account).await.unwrap();
    assert!(!summary.is_empty());
    
    // Step 4: Get current positions
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
            account: account.clone(),
        },
    ];
    client.set_positions(positions.clone()).await;
    
    let fetched_positions = client.get_positions(account).await.unwrap();
    assert_eq!(fetched_positions.len(), 2);
    
    // Step 5: Subscribe to market data for positions
    for position in &fetched_positions {
        let result = client.subscribe_market_data(1, &position.symbol).await;
        assert!(result.is_ok());
    }
    
    // Step 6: Place a new order
    let order = OrderRequest {
        symbol: "TSLA".to_string(),
        action: OrderAction::Buy,
        quantity: 10.0,
        order_type: OrderType::Limit,
        price: Some(200.0),
    };
    
    let order_result = client.place_order(order).await.unwrap();
    assert_eq!(order_result.status, "Submitted");
    assert_eq!(order_result.remaining, 10.0);
    
    // Step 7: Disconnect
    let disconnect_result = client.disconnect().await;
    assert!(disconnect_result.is_ok());
    assert!(!client.is_connected().await);
}

#[tokio::test]
async fn test_reconnection_handling() {
    // Test handling reconnection scenarios
    let client = MockIbkrClient::new();
    
    // Initial connection
    client.connect().await.unwrap();
    assert!(client.is_connected().await);
    
    // Simulate disconnection
    client.disconnect().await.unwrap();
    assert!(!client.is_connected().await);
    
    // Verify operations fail when disconnected
    let result = client.get_positions("DU123456").await;
    assert!(result.is_err());
    
    // Reconnect
    client.connect().await.unwrap();
    assert!(client.is_connected().await);
    
    // Verify operations work after reconnection
    let result = client.get_positions("DU123456").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_portfolio_value_calculation() {
    // Test calculating total portfolio value from positions
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();
    
    let positions = vec![
        Position {
            symbol: "AAPL".to_string(),
            position: 100.0,
            market_price: 150.0,
            market_value: 15000.0,
            average_cost: 145.0,
            unrealized_pnl: 500.0,
            realized_pnl: 100.0,
            account: "DU123456".to_string(),
        },
        Position {
            symbol: "GOOGL".to_string(),
            position: 20.0,
            market_price: 2500.0,
            market_value: 50000.0,
            average_cost: 2400.0,
            unrealized_pnl: 2000.0,
            realized_pnl: 0.0,
            account: "DU123456".to_string(),
        },
        Position {
            symbol: "MSFT".to_string(),
            position: 50.0,
            market_price: 300.0,
            market_value: 15000.0,
            average_cost: 310.0,
            unrealized_pnl: -500.0,
            realized_pnl: 200.0,
            account: "DU123456".to_string(),
        },
    ];
    
    client.set_positions(positions.clone()).await;
    let fetched_positions = client.get_positions("DU123456").await.unwrap();
    
    // Calculate totals
    let total_market_value: f64 = fetched_positions.iter()
        .map(|p| p.market_value)
        .sum();
    let total_unrealized_pnl: f64 = fetched_positions.iter()
        .map(|p| p.unrealized_pnl)
        .sum();
    let total_realized_pnl: f64 = fetched_positions.iter()
        .map(|p| p.realized_pnl)
        .sum();
    
    assert_eq!(total_market_value, 80000.0);
    assert_eq!(total_unrealized_pnl, 2000.0);
    assert_eq!(total_realized_pnl, 300.0);
}

#[tokio::test]
async fn test_order_types_handling() {
    // Test different order types
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();
    
    // Market order
    let market_order = OrderRequest {
        symbol: "AAPL".to_string(),
        action: OrderAction::Buy,
        quantity: 100.0,
        order_type: OrderType::Market,
        price: None,
    };
    let result = client.place_order(market_order).await;
    assert!(result.is_ok());
    
    // Limit order
    let limit_order = OrderRequest {
        symbol: "AAPL".to_string(),
        action: OrderAction::Sell,
        quantity: 50.0,
        order_type: OrderType::Limit,
        price: Some(155.0),
    };
    let result = client.place_order(limit_order).await;
    assert!(result.is_ok());
    
    // Stop order
    let stop_order = OrderRequest {
        symbol: "AAPL".to_string(),
        action: OrderAction::Sell,
        quantity: 100.0,
        order_type: OrderType::Stop,
        price: Some(145.0),
    };
    let result = client.place_order(stop_order).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_multiple_account_handling() {
    // Test handling multiple accounts
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();
    
    // Set multiple accounts
    let accounts = vec![
        "DU123456".to_string(),
        "DU789012".to_string(),
        "DU345678".to_string(),
    ];
    client.set_accounts(accounts.clone()).await;
    
    let fetched_accounts = client.get_accounts().await.unwrap();
    assert_eq!(fetched_accounts.len(), 3);
    
    // Test getting positions for each account
    for account in &fetched_accounts {
        let positions = client.get_positions(account).await;
        assert!(positions.is_ok());
    }
}