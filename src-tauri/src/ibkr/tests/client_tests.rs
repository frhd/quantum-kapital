use crate::ibkr::error::IbkrError;
use crate::ibkr::mocks::{test_fixtures, IbkrClientTrait, MockIbkrClient};

#[tokio::test]
async fn test_connect_success() {
    let client = MockIbkrClient::new();

    assert!(!client.is_connected().await);

    let result = client.connect().await;
    assert!(result.is_ok());
    assert!(client.is_connected().await);
}

#[tokio::test]
async fn test_connect_failure() {
    let client = MockIbkrClient::with_error(IbkrError::ConnectionFailed("Test error".to_string()));

    let result = client.connect().await;
    assert!(result.is_err());
    assert!(!client.is_connected().await);
}

#[tokio::test]
async fn test_disconnect() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    assert!(client.is_connected().await);

    let result = client.disconnect().await;
    assert!(result.is_ok());
    assert!(!client.is_connected().await);
}

#[tokio::test]
async fn test_get_accounts_when_connected() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let accounts = client.get_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0], "DU123456");
}

#[tokio::test]
async fn test_get_accounts_when_not_connected() {
    let client = MockIbkrClient::new();

    let result = client.get_accounts().await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IbkrError::NotConnected => {}
        _ => panic!("Expected NotConnected error"),
    }
}

#[tokio::test]
async fn test_get_positions_empty() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let positions = client.get_positions("DU123456").await.unwrap();
    assert_eq!(positions.len(), 0);
}

#[tokio::test]
async fn test_get_positions_with_data() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let sample_position = test_fixtures::sample_position();
    client.set_positions(vec![sample_position.clone()]).await;

    let positions = client.get_positions("DU123456").await.unwrap();
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].symbol, "AAPL");
    assert_eq!(positions[0].position, 100.0);
}

#[tokio::test]
async fn test_get_account_summary() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let sample_summary = test_fixtures::sample_account_summary();
    client.set_account_summary(sample_summary.clone()).await;

    let summary = client.get_account_summary("DU123456").await.unwrap();
    assert!(!summary.is_empty());

    let net_liq = summary.iter().find(|s| s.tag == "NetLiquidation");
    assert!(net_liq.is_some());
    assert_eq!(net_liq.unwrap().value, "100000.0");
}

#[tokio::test]
async fn test_place_order() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let order = test_fixtures::sample_order_request();
    let result = client.place_order(order).await.unwrap();

    assert_eq!(result.order_id, 12345);
    assert_eq!(result.status, "Submitted");
    assert_eq!(result.filled, 0.0);
    assert_eq!(result.remaining, 100.0);
}

#[tokio::test]
async fn test_subscribe_market_data() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();

    let result = client.subscribe_market_data(1, "AAPL").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_error_propagation() {
    let client = MockIbkrClient::new();
    client
        .set_error(Some(IbkrError::ApiError("API Error".to_string())))
        .await;

    let result = client.get_accounts().await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IbkrError::ApiError(msg) => assert_eq!(msg, "API Error"),
        _ => panic!("Expected ApiError"),
    }
}
