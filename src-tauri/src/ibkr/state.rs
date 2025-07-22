use crate::ibkr::client::IbkrClient;
use crate::ibkr::types::ConnectionConfig;
use std::sync::Arc;

pub struct IbkrState {
    pub client: Arc<IbkrClient>,
}

impl IbkrState {
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            client: Arc::new(IbkrClient::new(config)),
        }
    }
}
