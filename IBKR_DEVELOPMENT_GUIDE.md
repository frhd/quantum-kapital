# IBKR Development Guide - Test-Driven Approach

This guide outlines the test-driven development process for building IBKR (Interactive Brokers) features in the Quantum Kapital application.

## Development Philosophy

We follow a **Test-Driven Backend Development** approach where:
1. Tests are written before implementation
2. Backend functionality is fully tested before frontend integration
3. Mock clients simulate IBKR behavior for predictable testing
4. All edge cases and error scenarios are covered

## Project Structure

```
src-tauri/src/ibkr/
├── mod.rs           # Module exports
├── client.rs        # IBKR client implementation
├── commands.rs      # Tauri command handlers
├── types.rs         # Shared type definitions
├── state.rs         # Application state management
├── error.rs         # Error types and handling
├── mocks.rs         # Mock IBKR client for testing
└── tests/           # Test modules
    ├── mod.rs
    ├── client_tests.rs      # Unit tests for client
    ├── command_tests.rs     # Command flow tests
    └── integration_tests.rs # Full workflow tests
```

## Development Workflow

### 1. Define Types First

Before implementing any feature, define the data structures in `types.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewFeatureData {
    pub id: String,
    pub value: f64,
    // ... other fields
}
```

### 2. Write Tests Before Implementation

Create tests that define the expected behavior:

```rust
#[tokio::test]
async fn test_new_feature_success() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();
    
    // Define expected behavior
    let result = client.new_feature_method().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().value, expected_value);
}

#[tokio::test]
async fn test_new_feature_error_handling() {
    let client = MockIbkrClient::with_error(
        IbkrError::ApiError("Expected error".to_string())
    );
    
    let result = client.new_feature_method().await;
    assert!(result.is_err());
}
```

### 3. Implement Mock Behavior

Add the new method to the `IbkrClientTrait` and implement it in `MockIbkrClient`:

```rust
#[async_trait]
pub trait IbkrClientTrait: Send + Sync {
    // ... existing methods
    async fn new_feature_method(&self) -> Result<NewFeatureData>;
}

#[async_trait]
impl IbkrClientTrait for MockIbkrClient {
    async fn new_feature_method(&self) -> Result<NewFeatureData> {
        self.check_error().await?;
        if !self.is_connected().await {
            return Err(IbkrError::NotConnected);
        }
        
        // Return mock data
        Ok(NewFeatureData {
            id: "test_id".to_string(),
            value: 42.0,
        })
    }
}
```

### 4. Run Tests to Verify Mock

```bash
cargo test --manifest-path src-tauri/Cargo.toml ibkr::tests::
```

### 5. Implement Real Client

Only after tests pass with mocks, implement the real IBKR client:

```rust
impl IbkrClient {
    pub async fn new_feature_method(&self) -> Result<NewFeatureData> {
        // Real IBKR API implementation
    }
}
```

### 6. Create Tauri Command

Add the command handler in `commands.rs`:

```rust
#[tauri::command]
pub async fn ibkr_new_feature(
    state: tauri::State<'_, IbkrState>,
) -> Result<NewFeatureData, String> {
    state
        .get_client()
        .await?
        .new_feature_method()
        .await
        .map_err(|e| e.to_string())
}
```

### 7. Register Command

Add to the command list in `lib.rs`:

```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands
    ibkr::commands::ibkr_new_feature,
])
```

## Testing Best Practices

### Unit Tests
- Test individual methods in isolation
- Cover both success and failure cases
- Use descriptive test names

### Integration Tests
- Test complete workflows
- Simulate real trading scenarios
- Verify state transitions

### Mock Data Management

Use test fixtures for consistent test data:

```rust
pub mod test_fixtures {
    pub fn sample_complex_scenario() -> Vec<Position> {
        vec![
            // Profitable position
            Position { /* ... */ },
            // Loss position
            Position { /* ... */ },
            // Break-even position
            Position { /* ... */ },
        ]
    }
}
```

## Common Test Patterns

### Testing Connection States

```rust
#[tokio::test]
async fn test_operation_requires_connection() {
    let client = MockIbkrClient::new();
    
    // Should fail when not connected
    let result = client.some_operation().await;
    assert!(matches!(result, Err(IbkrError::NotConnected)));
    
    // Should succeed after connecting
    client.connect().await.unwrap();
    let result = client.some_operation().await;
    assert!(result.is_ok());
}
```

### Testing Error Propagation

```rust
#[tokio::test]
async fn test_error_handling_chain() {
    let client = MockIbkrClient::new();
    client.set_error(Some(IbkrError::ApiError("TWS Error".to_string()))).await;
    
    let result = client.place_order(order).await;
    match result {
        Err(IbkrError::ApiError(msg)) => assert!(msg.contains("TWS Error")),
        _ => panic!("Expected ApiError"),
    }
}
```

### Testing Concurrent Operations

```rust
#[tokio::test]
async fn test_concurrent_requests() {
    let client = MockIbkrClient::new();
    client.connect().await.unwrap();
    
    let futures = vec![
        client.get_positions("account1"),
        client.get_positions("account2"),
        client.get_positions("account3"),
    ];
    
    let results = futures::future::join_all(futures).await;
    assert!(results.iter().all(|r| r.is_ok()));
}
```

## Running Tests

```bash
# Run all IBKR tests
cargo test --manifest-path src-tauri/Cargo.toml ibkr::

# Run specific test module
cargo test --manifest-path src-tauri/Cargo.toml ibkr::tests::client_tests

# Run with output
cargo test --manifest-path src-tauri/Cargo.toml ibkr:: -- --nocapture

# Run single test
cargo test --manifest-path src-tauri/Cargo.toml test_connect_success
```

## Debugging IBKR Integration

### Enable Logging

The project uses `tracing` for structured logging:

```rust
// In your test
tracing_subscriber::fmt::init();

// In your code
tracing::info!("Connecting to IBKR at {}", url);
tracing::error!("Connection failed: {}", error);
```

### Capture Real IBKR Responses

When developing new features:

1. Create a debug command to capture real responses:
```rust
#[tauri::command]
pub async fn debug_capture_response(/* params */) -> String {
    // Make real IBKR call
    // Serialize response to JSON
    // Log or return for analysis
}
```

2. Use captured data to improve mocks:
```rust
// Save real response as test fixture
pub fn real_account_summary_response() -> Vec<AccountSummary> {
    // Paste captured JSON here
}
```

## Adding New IBKR Features

### Step-by-Step Process

1. **Research IBKR API**
   - Review IBKR API documentation
   - Understand request/response format
   - Note error conditions

2. **Design Types**
   - Create request/response types in `types.rs`
   - Add necessary enums and constants

3. **Write Comprehensive Tests**
   - Success scenarios
   - Error scenarios
   - Edge cases
   - Concurrent access

4. **Implement Mock**
   - Add to `IbkrClientTrait`
   - Implement in `MockIbkrClient`
   - Add test fixtures

5. **Run Tests with Mock**
   - Ensure all tests pass
   - Verify error handling

6. **Implement Real Client**
   - Add method to `IbkrClient`
   - Handle IBKR API specifics
   - Add retry logic if needed

7. **Create Tauri Command**
   - Add command handler
   - Map errors to strings
   - Register in `lib.rs`

8. **Integration Test**
   - Test with real TWS/Gateway
   - Verify in development mode
   - Document any quirks

## Common Pitfalls

1. **Not Testing Disconnection**
   - Always test operations when not connected
   - Verify reconnection behavior

2. **Ignoring Async Edge Cases**
   - Test concurrent operations
   - Handle race conditions

3. **Incomplete Error Handling**
   - Map all IBKR errors
   - Provide meaningful error messages

4. **Missing State Validation**
   - Verify state before operations
   - Clean up after errors

## Resources

- [IBKR API Documentation](https://interactivebrokers.github.io)
- [Rust ibapi Crate](https://docs.rs/ibapi)
- [Tokio Testing](https://tokio.rs/tokio/topics/testing)
- [Tauri Testing Guide](https://tauri.app/v1/guides/testing/)