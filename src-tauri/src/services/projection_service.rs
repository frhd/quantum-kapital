use crate::ibkr::error::Result;
use crate::ibkr::types::{
    CagrMetrics, FinancialProjection, FundamentalData, ProjectionAssumptions, ScenarioCagr,
    ScenarioProjections,
};

/// Service for calculating financial projections based on fundamental data
pub struct ProjectionService;

impl ProjectionService {
    /// Generate complete scenario projections (Bear/Base/Bull) from fundamental data
    pub fn generate_projections(
        fundamental: &FundamentalData,
        assumptions: &ProjectionAssumptions,
    ) -> Result<ScenarioProjections> {
        let current_year = 2025; // TODO: Use actual current year from chrono

        // Get the most recent historical data as baseline
        let baseline = fundamental
            .historical
            .last()
            .ok_or_else(|| crate::ibkr::error::IbkrError::Unknown("No historical data available".to_string()))?;

        // Generate projections for each scenario
        let bear = Self::generate_scenario_projection(
            baseline.revenue,
            baseline.net_income,
            baseline.revenue,
            fundamental.current_metrics.shares_outstanding,
            assumptions.bear_revenue_growth,
            assumptions.bear_margin_change,
            assumptions.pe_low,
            assumptions.pe_high,
            assumptions.shares_growth,
            current_year,
            assumptions.years,
        );

        let base = Self::generate_scenario_projection(
            baseline.revenue,
            baseline.net_income,
            baseline.revenue,
            fundamental.current_metrics.shares_outstanding,
            assumptions.base_revenue_growth,
            assumptions.base_margin_change,
            assumptions.pe_low,
            assumptions.pe_high,
            assumptions.shares_growth,
            current_year,
            assumptions.years,
        );

        let bull = Self::generate_scenario_projection(
            baseline.revenue,
            baseline.net_income,
            baseline.revenue,
            fundamental.current_metrics.shares_outstanding,
            assumptions.bull_revenue_growth,
            assumptions.bull_margin_change,
            assumptions.pe_low,
            assumptions.pe_high,
            assumptions.shares_growth,
            current_year,
            assumptions.years,
        );

        // Calculate CAGR for each scenario
        let bear_cagr = Self::calculate_cagr(&bear);
        let base_cagr = Self::calculate_cagr(&base);
        let bull_cagr = Self::calculate_cagr(&bull);

        Ok(ScenarioProjections {
            bear,
            base,
            bull,
            cagr: ScenarioCagr {
                bear: bear_cagr,
                base: base_cagr,
                bull: bull_cagr,
            },
        })
    }

    /// Generate projections for a single scenario
    fn generate_scenario_projection(
        initial_revenue: f64,
        initial_net_income: f64,
        _baseline_revenue: f64,
        initial_shares: f64,
        revenue_growth_rate: f64,
        margin_change_rate: f64,
        pe_low: f64,
        pe_high: f64,
        shares_growth_rate: f64,
        start_year: u32,
        num_years: u32,
    ) -> Vec<FinancialProjection> {
        let mut projections = Vec::new();
        let mut revenue = initial_revenue;
        let mut net_income = initial_net_income;
        let mut shares = initial_shares;
        let mut margin = (initial_net_income / initial_revenue) * 100.0; // Calculate initial margin

        let mut prev_net_income = initial_net_income;

        for year_offset in 0..num_years {
            let year = start_year + year_offset;

            // Apply growth rates
            if year_offset > 0 {
                revenue *= 1.0 + (revenue_growth_rate / 100.0);
                margin += margin_change_rate; // Add percentage points
                net_income = revenue * (margin / 100.0);
                shares *= 1.0 + (shares_growth_rate / 100.0);
            }

            let eps = net_income / shares * 1_000.0; // Convert from billions and millions to per share
            let share_price_low = eps * pe_low;
            let share_price_high = eps * pe_high;

            let net_income_growth = if year_offset == 0 {
                None
            } else {
                Some(((net_income - prev_net_income) / prev_net_income) * 100.0)
            };

            projections.push(FinancialProjection {
                year,
                revenue,
                revenue_growth: revenue_growth_rate,
                net_income,
                net_income_growth,
                net_income_margins: margin,
                eps,
                pe_low_est: pe_low,
                pe_high_est: pe_high,
                share_price_low,
                share_price_high,
            });

            prev_net_income = net_income;
        }

        projections
    }

    /// Calculate CAGR metrics for a projection scenario
    fn calculate_cagr(projections: &[FinancialProjection]) -> CagrMetrics {
        if projections.len() < 2 {
            return CagrMetrics {
                revenue: 0.0,
                share_price: 0.0,
            };
        }

        let first = &projections[0];
        let last = &projections[projections.len() - 1];
        let years = (last.year - first.year) as f64;

        // CAGR formula: ((End Value / Begin Value) ^ (1 / years)) - 1
        let revenue_cagr = ((last.revenue / first.revenue).powf(1.0 / years) - 1.0) * 100.0;

        // Use average of low and high for share price CAGR
        let first_price = (first.share_price_low + first.share_price_high) / 2.0;
        let last_price = (last.share_price_low + last.share_price_high) / 2.0;
        let share_price_cagr = ((last_price / first_price).powf(1.0 / years) - 1.0) * 100.0;

        CagrMetrics {
            revenue: revenue_cagr,
            share_price: share_price_cagr,
        }
    }

    /// Generate mock fundamental data for testing (will be replaced with real IBKR data)
    pub fn generate_mock_fundamental_data(symbol: &str) -> FundamentalData {
        use crate::ibkr::types::{AnalystEstimate, AnalystEstimates, CurrentMetrics, HistoricalFinancial};

        FundamentalData {
            symbol: symbol.to_string(),
            historical: vec![
                HistoricalFinancial {
                    year: 2021,
                    revenue: 26.97,
                    net_income: 3.76,
                    eps: 2.32,
                },
                HistoricalFinancial {
                    year: 2022,
                    revenue: 27.21,
                    net_income: 4.37,
                    eps: 2.69,
                },
                HistoricalFinancial {
                    year: 2023,
                    revenue: 26.97,
                    net_income: 4.79,
                    eps: 2.96,
                },
                HistoricalFinancial {
                    year: 2024,
                    revenue: 29.65,
                    net_income: 5.04,
                    eps: 3.12,
                },
            ],
            analyst_estimates: Some(AnalystEstimates {
                revenue: vec![
                    AnalystEstimate {
                        year: 2025,
                        estimate: 33.18,
                    },
                    AnalystEstimate {
                        year: 2026,
                        estimate: 38.50,
                    },
                ],
                eps: vec![
                    AnalystEstimate {
                        year: 2025,
                        estimate: 3.46,
                    },
                    AnalystEstimate {
                        year: 2026,
                        estimate: 4.50,
                    },
                ],
            }),
            current_metrics: CurrentMetrics {
                price: 138.50,
                pe_ratio: 44.0,
                shares_outstanding: 1620.0, // in millions
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_projections() {
        let fundamental = ProjectionService::generate_mock_fundamental_data("NVDA");
        let assumptions = ProjectionAssumptions::default();

        let projections = ProjectionService::generate_projections(&fundamental, &assumptions)
            .expect("Should generate projections");

        // Verify we have 5 years of projections for each scenario
        assert_eq!(projections.base.len(), 5);
        assert_eq!(projections.bear.len(), 5);
        assert_eq!(projections.bull.len(), 5);

        // Verify revenue growth in base case
        let base_first = &projections.base[0];
        let base_last = &projections.base[4];
        assert!(base_last.revenue > base_first.revenue);

        // Verify CAGR is calculated
        assert!(projections.cagr.base.revenue > 0.0);
        assert!(projections.cagr.base.share_price > 0.0);
    }

    #[test]
    fn test_cagr_calculation() {
        let projections = vec![
            FinancialProjection {
                year: 2025,
                revenue: 100.0,
                revenue_growth: 20.0,
                net_income: 20.0,
                net_income_growth: None,
                net_income_margins: 20.0,
                eps: 10.0,
                pe_low_est: 50.0,
                pe_high_est: 60.0,
                share_price_low: 500.0,
                share_price_high: 600.0,
            },
            FinancialProjection {
                year: 2030,
                revenue: 200.0,
                revenue_growth: 20.0,
                net_income: 40.0,
                net_income_growth: Some(20.0),
                net_income_margins: 20.0,
                eps: 20.0,
                pe_low_est: 50.0,
                pe_high_est: 60.0,
                share_price_low: 1000.0,
                share_price_high: 1200.0,
            },
        ];

        let cagr = ProjectionService::calculate_cagr(&projections);

        // CAGR for doubling over 5 years is approximately 14.87%
        assert!((cagr.revenue - 14.87).abs() < 0.1);
    }
}
