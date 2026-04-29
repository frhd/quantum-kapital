use std::sync::Arc;

use ibapi::contracts::Contract;

use crate::ibkr::error::{IbkrError, Result};

use super::IbkrClient;

impl IbkrClient {
    pub async fn subscribe_market_data(&self, symbol: &str) -> Result<()> {
        let client_clone = {
            let client = self.client.read().await;
            let client = client.as_ref().ok_or(IbkrError::NotConnected)?;
            Arc::clone(client)
        }; // Lock is dropped here!

        let symbol = symbol.to_string();

        tokio::task::spawn_blocking(move || {
            let contract = Contract::stock(&symbol).build();
            // For now, we'll request basic tick types
            let tick_types = &["233"]; // RTVolume
            match client_clone
                .market_data(&contract)
                .generic_ticks(tick_types)
                .subscribe()
            {
                Ok(_subscription) => {
                    // TODO: Store subscription and handle market data updates
                    Ok(())
                }
                Err(e) => Err(IbkrError::from(e)),
            }
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?
    }
}
