use super::types::{AuthState, SheetsError};
use google_sheets4::hyper::client::HttpConnector;
use google_sheets4::hyper_rustls::HttpsConnector;
use google_sheets4::oauth2::{
    authenticator::Authenticator, InstalledFlowAuthenticator, InstalledFlowReturnMethod,
};
use std::path::PathBuf;
use tokio::fs;

/// OAuth2 authenticator for Google Sheets API
#[derive(Clone)]
pub struct SheetsAuthenticator {
    auth: Option<Authenticator<HttpsConnector<HttpConnector>>>,
    credentials_path: PathBuf,
    token_cache_path: PathBuf,
}

impl SheetsAuthenticator {
    /// Create a new authenticator instance
    pub fn new() -> Result<Self, SheetsError> {
        // Get app data directory for storing credentials and tokens
        let config_dir = dirs::config_dir().ok_or_else(|| {
            SheetsError::ConfigError("Could not find config directory".to_string())
        })?;

        let app_dir = config_dir.join("quantum-kapital");

        Ok(Self {
            auth: None,
            credentials_path: app_dir.join("google_credentials.json"),
            token_cache_path: app_dir.join("google_token_cache.json"),
        })
    }

    /// Check if credentials file exists
    pub async fn has_credentials(&self) -> bool {
        self.credentials_path.exists()
    }

    /// Save OAuth2 credentials from user input
    pub async fn save_credentials(&self, credentials_json: &str) -> Result<(), SheetsError> {
        // Ensure the directory exists
        if let Some(parent) = self.credentials_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Validate JSON before saving
        serde_json::from_str::<serde_json::Value>(credentials_json)
            .map_err(|e| SheetsError::ConfigError(format!("Invalid credentials JSON: {e}")))?;

        // Save credentials to file
        fs::write(&self.credentials_path, credentials_json).await?;

        Ok(())
    }

    /// Authenticate with Google using OAuth2 flow
    pub async fn authenticate(
        &mut self,
    ) -> Result<Authenticator<HttpsConnector<HttpConnector>>, SheetsError> {
        if !self.has_credentials().await {
            return Err(SheetsError::AuthError(
                "Google credentials not found. Please configure OAuth2 credentials first."
                    .to_string(),
            ));
        }

        // Read the credentials file
        let secret = google_sheets4::oauth2::read_application_secret(&self.credentials_path)
            .await
            .map_err(|e| SheetsError::AuthError(format!("Failed to read credentials: {e}")))?;

        // Create authenticator with installed flow (opens browser for user consent)
        let auth =
            InstalledFlowAuthenticator::builder(secret, InstalledFlowReturnMethod::HTTPRedirect)
                .persist_tokens_to_disk(&self.token_cache_path)
                .build()
                .await
                .map_err(|e| {
                    SheetsError::AuthError(format!("Failed to create authenticator: {e}"))
                })?;

        self.auth = Some(auth.clone());
        Ok(auth)
    }

    /// Get current authentication state
    pub async fn get_auth_state(&self) -> AuthState {
        AuthState {
            authenticated: self.auth.is_some(),
            user_email: None, // TODO: Extract from token if needed
        }
    }

    /// Clear authentication (logout)
    pub async fn clear_auth(&mut self) -> Result<(), SheetsError> {
        self.auth = None;

        // Remove token cache file
        if self.token_cache_path.exists() {
            fs::remove_file(&self.token_cache_path).await?;
        }

        Ok(())
    }
}

impl Default for SheetsAuthenticator {
    fn default() -> Self {
        Self::new().expect("Failed to create default authenticator")
    }
}
