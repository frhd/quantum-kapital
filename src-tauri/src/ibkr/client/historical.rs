use ibapi::contracts::Contract;

use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::historical::{
    BarSize as OurBarSize, HistoricalBar, HistoricalDataRequest, WhatToShow as OurWhatToShow,
};

use super::IbkrClient;

impl IbkrClient {
    /// Fetches historical bars from IBKR. Real-IBKR live integration is
    /// not exercised by unit tests in Phase 02 — service tests use a
    /// mock fetcher implementing `HistoricalDataFetcher`. The body here
    /// translates our domain types to ibapi 2's enums and runs the
    /// blocking call on `spawn_blocking` like the rest of this module.
    pub async fn get_historical_data(
        &self,
        request: HistoricalDataRequest,
    ) -> Result<Vec<HistoricalBar>> {
        use ibapi::market_data::historical::{
            BarSize as IbBarSize, Duration as IbDuration, WhatToShow as IbWhatToShow,
        };
        use ibapi::market_data::TradingHours;

        let client_clone = self.ibapi_client().await?;

        let bars = tokio::task::spawn_blocking(move || -> Result<Vec<HistoricalBar>> {
            let contract = Contract::stock(&request.symbol).build();
            let ib_bar = match request.bar_size {
                OurBarSize::Sec1 => IbBarSize::Sec,
                OurBarSize::Sec5 => IbBarSize::Sec5,
                OurBarSize::Sec15 => IbBarSize::Sec15,
                OurBarSize::Sec30 => IbBarSize::Sec30,
                OurBarSize::Min1 => IbBarSize::Min,
                OurBarSize::Min2 => IbBarSize::Min2,
                OurBarSize::Min3 => IbBarSize::Min3,
                OurBarSize::Min5 => IbBarSize::Min5,
                OurBarSize::Min15 => IbBarSize::Min15,
                OurBarSize::Min20 => IbBarSize::Min20,
                OurBarSize::Min30 => IbBarSize::Min30,
                OurBarSize::Hour1 => IbBarSize::Hour,
                OurBarSize::Day1 => IbBarSize::Day,
            };
            let ib_what = match request.what_to_show {
                OurWhatToShow::Trades => IbWhatToShow::Trades,
                OurWhatToShow::Midpoint => IbWhatToShow::MidPoint,
                OurWhatToShow::Bid => IbWhatToShow::Bid,
                OurWhatToShow::Ask => IbWhatToShow::Ask,
                OurWhatToShow::BidAsk => IbWhatToShow::BidAsk,
                OurWhatToShow::HistoricalVolatility => IbWhatToShow::HistoricalVolatility,
                OurWhatToShow::OptionImpliedVolatility => IbWhatToShow::OptionImpliedVolatility,
            };

            // Parse our "{N} {UNIT}" duration string back into ibapi's Duration.
            // We only emit "{N} D" from the service so this is the common path.
            let ib_duration: IbDuration = request.duration.parse().map_err(|e| {
                IbkrError::RequestFailed(format!(
                    "invalid duration string '{}': {e}",
                    request.duration
                ))
            })?;

            let trading_hours = if request.use_rth {
                TradingHours::Regular
            } else {
                TradingHours::Extended
            };

            // We pass `None` for end_date_time and let IBKR default to "now".
            // The end_date_time string in our request type is informational
            // for now; a future revision can route it through OffsetDateTime.
            let data = client_clone
                .historical_data(&contract, None, ib_duration, ib_bar, ib_what, trading_hours)
                .map_err(IbkrError::from)?;

            Ok(data
                .bars
                .into_iter()
                .map(|b| {
                    let ts = b.date.unix_timestamp();
                    let chrono_dt =
                        chrono::DateTime::from_timestamp(ts, 0).unwrap_or_else(chrono::Utc::now);
                    let formatted = if request.bar_size == OurBarSize::Day1 {
                        chrono_dt.format("%Y%m%d").to_string()
                    } else {
                        chrono_dt.format("%Y%m%d %H:%M:%S").to_string()
                    };
                    HistoricalBar {
                        time: formatted,
                        open: b.open,
                        high: b.high,
                        low: b.low,
                        close: b.close,
                        volume: b.volume as i64,
                        wap: b.wap,
                        count: b.count,
                    }
                })
                .collect())
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))??;

        Ok(bars)
    }
}
