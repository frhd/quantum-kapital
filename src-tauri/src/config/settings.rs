use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

use crate::strategies::DetectorsConfig;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub ibkr: IbkrConfig,
    pub logging: LoggingConfig,
    pub ui: UiConfig,
    pub api: ApiConfig,
    #[serde(default)]
    pub tracker: TrackerConfig,
    #[serde(default)]
    pub detectors: DetectorsConfig,
    #[serde(default)]
    pub auto_scanner: AutoScannerConfig,
    #[serde(default)]
    pub social_sentiment: SocialSentimentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbkrConfig {
    pub default_host: String,
    pub default_port: u16,
    pub default_client_id: i32,
    pub connection_timeout_ms: u64,
    pub reconnect_interval_ms: u64,
    pub max_reconnect_attempts: u32,
    pub rate_limit_per_second: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file_path: Option<PathBuf>,
    pub max_file_size_mb: u64,
    pub max_files: u32,
    pub console_output: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: String,
    pub default_refresh_interval_ms: u64,
    pub show_notifications: bool,
    pub auto_save_layout: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub alpha_vantage_api_key: Option<String>, // Alpha Vantage API key
    #[serde(default)]
    pub anthropic_api_key: Option<String>, // Anthropic API key
    #[serde(default = "default_daily_llm_budget_usd")]
    pub daily_llm_budget_usd: f64,
}

pub fn default_daily_llm_budget_usd() -> f64 {
    5.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerConfig {
    /// Cadence for the Phase 14 intraday scheduler. Default 300s
    /// (5 min). Tunable via Phase 22 detector-config UI.
    pub intraday_tick_interval_secs: u64,
}

/// Configuration for the scheduled auto-scanner (first automation step).
///
/// Ships dark — `enabled` defaults to `false`, so adding the field to a
/// pre-existing settings file has no effect until the user opts in. The
/// service runs each [`ScanProfile`] in `profiles`, plus one
/// industry-filtered TOP_PERC_GAIN profile per entry in `industries`,
/// promoting the top rows that pass the per-profile price/volume filters
/// into the watchlist with `source = auto_scanner`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoScannerConfig {
    pub enabled: bool,
    /// Minimum minutes between successful runs (the scheduler polls
    /// every minute and skips ticks inside this window).
    pub interval_minutes: u32,
    /// Hard cap across all profiles per UTC day. Once hit, further
    /// promotions in the same day are skipped (and logged).
    pub daily_cap: u32,
    pub profiles: Vec<ScanProfile>,
    /// Industries to track ahead of broad-market hot lists. Each value
    /// expands at runtime into one TOP_PERC_GAIN profile filtered by
    /// `industryLike`.
    pub industries: Vec<String>,
    /// Phase 4 — auto-promotion gate. Candidates whose merged
    /// [`crate::services::candidate_universe::Candidate::score`]
    /// crosses this threshold are added straight to `tracked_tickers`
    /// (capped per profile by `promote_top_k` and per day by
    /// `daily_cap`). Below the threshold they stay in
    /// `candidate_universe` for the agent's `promote_candidate` review
    /// path. `0.0` = promote everything that passes the per-profile
    /// cap (legacy behaviour); raise it as you trust the scoring.
    #[serde(default = "default_auto_promote_threshold")]
    pub auto_promote_threshold: f64,
}

pub fn default_auto_promote_threshold() -> f64 {
    0.7
}

/// Single scan invocation. Maps almost 1:1 to
/// [`crate::ibkr::types::ScannerSubscription`] but adds promotion and
/// labelling fields the IBKR-side struct doesn't carry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProfile {
    /// Free-form label persisted in `tracked_tickers.source_meta` so
    /// the UI can surface where each auto-add came from.
    pub name: String,
    pub scan_code: String,
    #[serde(default = "default_location_code")]
    pub location_code: String,
    #[serde(default)]
    pub above_price: Option<f64>,
    #[serde(default)]
    pub above_volume: Option<i32>,
    #[serde(default)]
    pub industry_filter: Option<String>,
    /// Number of top-ranked rows to promote per run.
    pub promote_top_k: usize,
    #[serde(default = "default_number_of_rows")]
    pub number_of_rows: i32,
}

fn default_location_code() -> String {
    "STK.US.MAJOR".to_string()
}

fn default_number_of_rows() -> i32 {
    25
}

impl AutoScannerConfig {
    /// Seeds the broad-market profiles so `Default::default()` covers
    /// the Phase-4 "5+ sources in the morning candidate set" target out
    /// of the box: top % gainers, top % losers, hot-by-volume, most-
    /// active, and a 52-week-high breakout proxy. Each carries
    /// reasonable price/volume floors so penny-stock noise stays out
    /// of the candidate universe. Sentiment-surge candidates are
    /// produced by [`crate::services::sentiment_surge_scanner`] and
    /// don't appear here — they're a synthetic source, not an IBKR
    /// scan.
    fn default_broad_profiles() -> Vec<ScanProfile> {
        vec![
            ScanProfile {
                name: "Top % Gainers".to_string(),
                scan_code: "TOP_PERC_GAIN".to_string(),
                location_code: default_location_code(),
                above_price: Some(5.0),
                above_volume: Some(500_000),
                industry_filter: None,
                promote_top_k: 5,
                number_of_rows: default_number_of_rows(),
            },
            ScanProfile {
                name: "Top % Losers".to_string(),
                scan_code: "TOP_PERC_LOSE".to_string(),
                location_code: default_location_code(),
                above_price: Some(5.0),
                above_volume: Some(500_000),
                industry_filter: None,
                promote_top_k: 3,
                number_of_rows: default_number_of_rows(),
            },
            ScanProfile {
                name: "Hot by Volume".to_string(),
                scan_code: "HOT_BY_VOLUME".to_string(),
                location_code: default_location_code(),
                above_price: Some(5.0),
                above_volume: None,
                industry_filter: None,
                promote_top_k: 5,
                number_of_rows: default_number_of_rows(),
            },
            ScanProfile {
                name: "Most Active".to_string(),
                scan_code: "MOST_ACTIVE".to_string(),
                location_code: default_location_code(),
                above_price: Some(5.0),
                above_volume: None,
                industry_filter: None,
                promote_top_k: 3,
                number_of_rows: default_number_of_rows(),
            },
            ScanProfile {
                name: "52-Week Highs".to_string(),
                // IBKR exposes this as `HIGH_VS_52W_HL` (% off the
                // 52-week high/low). Newly-confirmed breakouts cluster
                // at the top of the list.
                scan_code: "HIGH_VS_52W_HL".to_string(),
                location_code: default_location_code(),
                above_price: Some(10.0),
                above_volume: Some(500_000),
                industry_filter: None,
                promote_top_k: 3,
                number_of_rows: default_number_of_rows(),
            },
        ]
    }

    /// Effective profile list = explicit `profiles` ∪ derived
    /// industry-filtered TOP_PERC_GAIN profiles for each entry in
    /// `industries`. The runtime calls this each tick so editing
    /// `industries` in the JSON propagates without restarting.
    pub fn effective_profiles(&self) -> Vec<ScanProfile> {
        let mut out = self.profiles.clone();
        for industry in &self.industries {
            out.push(ScanProfile {
                name: format!("{industry} momentum"),
                scan_code: "TOP_PERC_GAIN".to_string(),
                location_code: default_location_code(),
                above_price: Some(5.0),
                above_volume: None,
                industry_filter: Some(industry.clone()),
                promote_top_k: 3,
                number_of_rows: default_number_of_rows(),
            });
        }
        out
    }
}

/// Knobs for the [`SocialSentimentScheduler`] and per-provider enable
/// flags. Ships dark: `enabled` defaults to `false`. Reddit auth uses
/// public-JSON in v1 — the `reddit_*` fields are placeholders for a
/// future OAuth backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialSentimentConfig {
    pub enabled: bool,
    /// Minimum minutes between successful fetches. The scheduler still
    /// polls every 60s but skips ticks inside this window.
    pub min_interval_minutes: u32,
    pub source_apewisdom_enabled: bool,
    pub source_stocktwits_enabled: bool,
    pub source_reddit_enabled: bool,
    /// Optional Reddit OAuth credentials. Unused by the v1 public-JSON
    /// path; serialised so settings.json can carry them for the future
    /// PRAW / `roux` backend without a second migration.
    #[serde(default)]
    pub reddit_client_id: Option<String>,
    #[serde(default)]
    pub reddit_client_secret: Option<String>,
}

impl Default for SocialSentimentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_interval_minutes: 60,
            source_apewisdom_enabled: true,
            source_stocktwits_enabled: true,
            source_reddit_enabled: true,
            reddit_client_id: std::env::var("REDDIT_CLIENT_ID").ok(),
            reddit_client_secret: std::env::var("REDDIT_CLIENT_SECRET").ok(),
        }
    }
}

impl Default for AutoScannerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 30,
            daily_cap: 10,
            profiles: Self::default_broad_profiles(),
            industries: Vec::new(),
            auto_promote_threshold: default_auto_promote_threshold(),
        }
    }
}

impl Default for IbkrConfig {
    fn default() -> Self {
        Self {
            default_host: "127.0.0.1".to_string(),
            default_port: 4004,
            default_client_id: 100,
            connection_timeout_ms: 30000,
            reconnect_interval_ms: 5000,
            max_reconnect_attempts: 3,
            rate_limit_per_second: 50,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file_path: None,
            max_file_size_mb: 10,
            max_files: 5,
            console_output: true,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            default_refresh_interval_ms: 1000,
            show_notifications: true,
            auto_save_layout: true,
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            alpha_vantage_api_key: std::env::var("ALPHA_VANTAGE_API_KEY").ok(),
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            daily_llm_budget_usd: default_daily_llm_budget_usd(),
        }
    }
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            intraday_tick_interval_secs: 300,
        }
    }
}

#[allow(dead_code)]
impl AppConfig {
    /// Get the path to the settings file
    pub fn settings_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let config_dir = dirs::config_dir().ok_or("Could not find config directory")?;
        let app_dir = config_dir.join("quantum-kapital");
        Ok(app_dir.join("settings.json"))
    }

    /// Load settings from disk, or return defaults if file doesn't exist
    pub async fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let settings_path = Self::settings_path()?;

        if settings_path.exists() {
            let contents = fs::read_to_string(&settings_path).await?;
            let config: AppConfig = serde_json::from_str(&contents)?;
            Ok(config)
        } else {
            // Return default settings if file doesn't exist
            Ok(Self::default())
        }
    }

    /// Save settings to disk
    pub async fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let settings_path = Self::settings_path()?;

        // Ensure directory exists
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Serialize settings with pretty formatting
        let json = serde_json::to_string_pretty(self)?;

        // Write to file
        fs::write(&settings_path, json).await?;

        Ok(())
    }

    /// Load synchronously (for initial app setup)
    pub fn load_sync() -> Result<Self, Box<dyn std::error::Error>> {
        let settings_path = Self::settings_path()?;

        if settings_path.exists() {
            let contents = std::fs::read_to_string(&settings_path)?;
            let config: AppConfig = serde_json::from_str(&contents)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_scanner_defaults_are_disabled_with_broad_profiles() {
        let cfg = AutoScannerConfig::default();
        assert!(!cfg.enabled, "auto-scanner ships dark");
        assert_eq!(cfg.interval_minutes, 30);
        assert_eq!(cfg.daily_cap, 10);
        assert!(cfg.industries.is_empty());
        let scan_codes: Vec<&str> = cfg.profiles.iter().map(|p| p.scan_code.as_str()).collect();
        assert_eq!(
            scan_codes,
            vec![
                "TOP_PERC_GAIN",
                "TOP_PERC_LOSE",
                "HOT_BY_VOLUME",
                "MOST_ACTIVE",
                "HIGH_VS_52W_HL",
            ]
        );
        assert!(cfg.profiles.iter().all(|p| p.industry_filter.is_none()));
        // Phase 4 default: 0.7 score threshold for auto-promotion.
        assert!((cfg.auto_promote_threshold - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn effective_profiles_appends_one_per_industry() {
        let cfg = AutoScannerConfig {
            industries: vec!["Semiconductors".to_string(), "Biotechnology".to_string()],
            ..Default::default()
        };
        let effective = cfg.effective_profiles();
        // 5 broad + 2 industry-filtered.
        assert_eq!(effective.len(), 7);
        let industry_filtered: Vec<&Option<String>> = effective
            .iter()
            .filter(|p| p.industry_filter.is_some())
            .map(|p| &p.industry_filter)
            .collect();
        assert_eq!(industry_filtered.len(), 2);
        assert_eq!(industry_filtered[0].as_deref(), Some("Semiconductors"));
        assert_eq!(industry_filtered[1].as_deref(), Some("Biotechnology"));
        // Industry-derived profiles all use TOP_PERC_GAIN per the plan.
        assert!(effective
            .iter()
            .filter(|p| p.industry_filter.is_some())
            .all(|p| p.scan_code == "TOP_PERC_GAIN"));
    }

    #[test]
    fn app_config_round_trips_through_json_with_auto_scanner_default() {
        // Existing settings files predate the field; the `#[serde(default)]`
        // attribute on `AppConfig.auto_scanner` must keep them parseable.
        let pre_existing = r#"{
            "ibkr": {
                "default_host": "127.0.0.1",
                "default_port": 4004,
                "default_client_id": 100,
                "connection_timeout_ms": 30000,
                "reconnect_interval_ms": 5000,
                "max_reconnect_attempts": 3,
                "rate_limit_per_second": 50
            },
            "logging": {
                "level": "info",
                "file_path": null,
                "max_file_size_mb": 10,
                "max_files": 5,
                "console_output": true
            },
            "ui": {
                "theme": "dark",
                "default_refresh_interval_ms": 1000,
                "show_notifications": true,
                "auto_save_layout": true
            },
            "api": { "alpha_vantage_api_key": null },
            "tracker": { "intraday_tick_interval_secs": 300 },
            "detectors": {}
        }"#;
        let cfg: AppConfig = serde_json::from_str(pre_existing).unwrap();
        assert!(!cfg.auto_scanner.enabled);
        assert_eq!(cfg.auto_scanner.profiles.len(), 5);
        // The new field round-trips with its default when absent from JSON.
        assert!((cfg.auto_scanner.auto_promote_threshold - 0.7).abs() < f64::EPSILON);
    }
}
