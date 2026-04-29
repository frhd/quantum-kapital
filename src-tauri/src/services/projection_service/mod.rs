use crate::ibkr::error::Result;
use crate::ibkr::types::{
    FinancialProjection, FundamentalData, ProjectionAssumptions, ProjectionResults, ScenarioCagr,
    ScenarioProjections, YearlyProjection,
};

mod scenarios;

use scenarios::{calculate_cagr, generate_scenario_projection, ScenarioParams};

/// Service for calculating financial projections based on fundamental data
pub struct ProjectionService;

impl ProjectionService {
    /// Generate complete scenario projections (Bear/Base/Bull) from fundamental data
    pub fn generate_projections(
        fundamental: &FundamentalData,
        assumptions: &ProjectionAssumptions,
    ) -> Result<ScenarioProjections> {
        // Get the most recent historical data as baseline
        let baseline = fundamental.historical.last().ok_or_else(|| {
            crate::ibkr::error::IbkrError::Unknown("No historical data available".to_string())
        })?;

        // Start projections from the year after the baseline
        // This ensures we don't mislabel historical data as projections
        let projection_start_year = baseline.year + 1;

        // Generate projections for each scenario
        let bear = generate_scenario_projection(ScenarioParams {
            initial_revenue: baseline.revenue,
            initial_net_income: baseline.net_income,
            initial_shares: fundamental.current_metrics.shares_outstanding,
            revenue_growth_rate: assumptions.bear_revenue_growth,
            margin_change_rate: assumptions.bear_margin_change,
            pe_low: assumptions.pe_low,
            pe_high: assumptions.pe_high,
            ps_low: assumptions.ps_low,
            ps_high: assumptions.ps_high,
            shares_growth_rate: assumptions.shares_growth,
            start_year: projection_start_year,
            num_years: assumptions.years,
            analyst_estimates: fundamental.analyst_estimates.as_ref(),
        });

        let base = generate_scenario_projection(ScenarioParams {
            initial_revenue: baseline.revenue,
            initial_net_income: baseline.net_income,
            initial_shares: fundamental.current_metrics.shares_outstanding,
            revenue_growth_rate: assumptions.base_revenue_growth,
            margin_change_rate: assumptions.base_margin_change,
            pe_low: assumptions.pe_low,
            pe_high: assumptions.pe_high,
            ps_low: assumptions.ps_low,
            ps_high: assumptions.ps_high,
            shares_growth_rate: assumptions.shares_growth,
            start_year: projection_start_year,
            num_years: assumptions.years,
            analyst_estimates: fundamental.analyst_estimates.as_ref(),
        });

        let bull = generate_scenario_projection(ScenarioParams {
            initial_revenue: baseline.revenue,
            initial_net_income: baseline.net_income,
            initial_shares: fundamental.current_metrics.shares_outstanding,
            revenue_growth_rate: assumptions.bull_revenue_growth,
            margin_change_rate: assumptions.bull_margin_change,
            pe_low: assumptions.pe_low,
            pe_high: assumptions.pe_high,
            ps_low: assumptions.ps_low,
            ps_high: assumptions.ps_high,
            shares_growth_rate: assumptions.shares_growth,
            start_year: projection_start_year,
            num_years: assumptions.years,
            analyst_estimates: fundamental.analyst_estimates.as_ref(),
        });

        // Calculate CAGR for each scenario
        let bear_cagr = calculate_cagr(&bear);
        let base_cagr = calculate_cagr(&base);
        let bull_cagr = calculate_cagr(&bull);

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

    /// Generate projection results grouped by year (baseline + forward projections)
    /// This is the preferred format for displaying projections in the UI
    pub fn generate_projection_results(
        fundamental: &FundamentalData,
        assumptions: &ProjectionAssumptions,
    ) -> Result<ProjectionResults> {
        // Get the most recent historical data as baseline
        let baseline_data = fundamental.historical.last().ok_or_else(|| {
            crate::ibkr::error::IbkrError::Unknown("No historical data available".to_string())
        })?;

        // Create baseline projection (actual data, not a projection)
        let baseline = Self::create_baseline_projection(
            baseline_data,
            fundamental.current_metrics.shares_outstanding,
            fundamental.current_metrics.price,
            assumptions,
        );

        // Start projections from the year after the baseline
        let projection_start_year = baseline_data.year + 1;

        // Generate projections for all three scenarios
        let bear = generate_scenario_projection(ScenarioParams {
            initial_revenue: baseline_data.revenue,
            initial_net_income: baseline_data.net_income,
            initial_shares: fundamental.current_metrics.shares_outstanding,
            revenue_growth_rate: assumptions.bear_revenue_growth,
            margin_change_rate: assumptions.bear_margin_change,
            pe_low: assumptions.pe_low,
            pe_high: assumptions.pe_high,
            ps_low: assumptions.ps_low,
            ps_high: assumptions.ps_high,
            shares_growth_rate: assumptions.shares_growth,
            start_year: projection_start_year,
            num_years: assumptions.years,
            analyst_estimates: fundamental.analyst_estimates.as_ref(),
        });

        let base = generate_scenario_projection(ScenarioParams {
            initial_revenue: baseline_data.revenue,
            initial_net_income: baseline_data.net_income,
            initial_shares: fundamental.current_metrics.shares_outstanding,
            revenue_growth_rate: assumptions.base_revenue_growth,
            margin_change_rate: assumptions.base_margin_change,
            pe_low: assumptions.pe_low,
            pe_high: assumptions.pe_high,
            ps_low: assumptions.ps_low,
            ps_high: assumptions.ps_high,
            shares_growth_rate: assumptions.shares_growth,
            start_year: projection_start_year,
            num_years: assumptions.years,
            analyst_estimates: fundamental.analyst_estimates.as_ref(),
        });

        let bull = generate_scenario_projection(ScenarioParams {
            initial_revenue: baseline_data.revenue,
            initial_net_income: baseline_data.net_income,
            initial_shares: fundamental.current_metrics.shares_outstanding,
            revenue_growth_rate: assumptions.bull_revenue_growth,
            margin_change_rate: assumptions.bull_margin_change,
            pe_low: assumptions.pe_low,
            pe_high: assumptions.pe_high,
            ps_low: assumptions.ps_low,
            ps_high: assumptions.ps_high,
            shares_growth_rate: assumptions.shares_growth,
            start_year: projection_start_year,
            num_years: assumptions.years,
            analyst_estimates: fundamental.analyst_estimates.as_ref(),
        });

        // Group projections by year
        let mut projections = Vec::new();
        for i in 0..assumptions.years as usize {
            if let (Some(bear_proj), Some(base_proj), Some(bull_proj)) =
                (bear.get(i), base.get(i), bull.get(i))
            {
                projections.push(YearlyProjection {
                    year: bear_proj.year,
                    bear: bear_proj.clone(),
                    base: base_proj.clone(),
                    bull: bull_proj.clone(),
                });
            }
        }

        // Calculate CAGR for each scenario
        let bear_cagr = calculate_cagr(&bear);
        let base_cagr = calculate_cagr(&base);
        let bull_cagr = calculate_cagr(&bull);

        Ok(ProjectionResults {
            baseline,
            projections,
            cagr: ScenarioCagr {
                bear: bear_cagr,
                base: base_cagr,
                bull: bull_cagr,
            },
        })
    }

    /// Create a baseline projection from actual historical data
    fn create_baseline_projection(
        baseline_data: &crate::ibkr::types::HistoricalFinancial,
        shares_outstanding: f64,
        current_price: f64,
        _assumptions: &ProjectionAssumptions,
    ) -> FinancialProjection {
        let eps = baseline_data.eps;
        let margin = (baseline_data.net_income / baseline_data.revenue) * 100.0;

        // For baseline, we use actual price and calculate implied P/E
        let pe_ratio = if eps > 0.0 { current_price / eps } else { 0.0 };

        FinancialProjection {
            year: baseline_data.year,
            revenue: baseline_data.revenue,
            revenue_growth: 0.0, // Historical, not a projection
            net_income: baseline_data.net_income,
            net_income_growth: None,
            net_income_margins: margin,
            eps,
            pe_low_est: pe_ratio,
            pe_high_est: pe_ratio,
            share_price_low: current_price,
            share_price_high: current_price,
            valuation_method: if eps > 0.0 {
                "P/E".to_string()
            } else {
                "P/S".to_string()
            },
            ps_low_est: if eps < 0.0 {
                Some(current_price / (baseline_data.revenue / shares_outstanding * 1_000.0))
            } else {
                None
            },
            ps_high_est: if eps < 0.0 {
                Some(current_price / (baseline_data.revenue / shares_outstanding * 1_000.0))
            } else {
                None
            },
            analyst_eps_estimate: None, // Baseline is actual, not estimated
        }
    }

    /// Generate mock fundamental data for testing (will be replaced with real IBKR data)
    /// Updated with current NVDA data as of November 2025
    pub fn generate_mock_fundamental_data(symbol: &str) -> FundamentalData {
        use crate::ibkr::types::{
            AnalystEstimate, AnalystEstimates, CurrentMetrics, HistoricalFinancial,
        };

        FundamentalData {
            symbol: symbol.to_string(),
            historical: vec![
                HistoricalFinancial {
                    year: 2021,
                    revenue: 26.91,
                    net_income: 9.75,
                    eps: 3.85,
                },
                HistoricalFinancial {
                    year: 2022,
                    revenue: 26.97,
                    net_income: 4.37,
                    eps: 0.17,
                },
                HistoricalFinancial {
                    year: 2023,
                    revenue: 60.92,
                    net_income: 29.76,
                    eps: 1.19,
                },
                HistoricalFinancial {
                    year: 2024,
                    revenue: 130.50,
                    net_income: 72.88,
                    eps: 2.94,
                },
            ],
            analyst_estimates: Some(AnalystEstimates {
                revenue: vec![
                    AnalystEstimate {
                        year: 2025,
                        estimate: 170.8,
                    },
                    AnalystEstimate {
                        year: 2026,
                        estimate: 195.0,
                    },
                ],
                eps: vec![
                    AnalystEstimate {
                        year: 2025,
                        estimate: 3.50,
                    },
                    AnalystEstimate {
                        year: 2026,
                        estimate: 4.25,
                    },
                ],
            }),
            current_metrics: CurrentMetrics {
                price: 202.49,
                pe_ratio: 68.9,
                shares_outstanding: 24804.0, // in millions (24.804B shares)
                name: Some(format!("{symbol} Corporation")),
                exchange: Some("NASDAQ".to_string()),
                market_cap: Some("5.0T".to_string()),
                dividend_yield: Some(0.03),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::types::{CurrentMetrics, HistoricalFinancial};

    /// Regression: catches an accidental `pub` → `pub(crate)` slip on the
    /// `ProjectionService` API after the Phase 25 split into
    /// `projection_service/{mod,scenarios}.rs`. Imports the type by its
    /// public crate path and runs `generate_projections` against the
    /// existing mock fixture.
    #[test]
    fn projection_service_split_compiles() {
        use crate::services::projection_service::{ProjectionAssumptions, ProjectionService};
        let fundamental = ProjectionService::generate_mock_fundamental_data("NVDA");
        let assumptions = ProjectionAssumptions::default();
        let _projections = ProjectionService::generate_projections(&fundamental, &assumptions)
            .expect("mock fundamental fixture must produce projections");
    }

    #[test]
    fn test_generate_projections() {
        let fundamental = ProjectionService::generate_mock_fundamental_data("NVDA");
        let assumptions = ProjectionAssumptions::default();

        let projections = ProjectionService::generate_projections(&fundamental, &assumptions)
            .expect("mock fundamental fixture must produce projections");

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
                valuation_method: "P/E".to_string(),
                ps_low_est: None,
                ps_high_est: None,
                analyst_eps_estimate: None,
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
                valuation_method: "P/E".to_string(),
                ps_low_est: None,
                ps_high_est: None,
                analyst_eps_estimate: None,
            },
        ];

        let cagr = calculate_cagr(&projections);

        // CAGR for doubling over 5 years is approximately 14.87%
        assert!((cagr.revenue - 14.87).abs() < 0.1);
    }

    #[test]
    fn test_negative_eps_uses_ps_valuation() {
        // Test that companies with negative EPS use P/S valuation
        let fundamental = FundamentalData {
            symbol: "LOSSMAKER".to_string(),
            historical: vec![HistoricalFinancial {
                year: 2024,
                revenue: 10.0,    // $10B revenue
                net_income: -2.0, // Losing $2B
                eps: -0.5,        // Negative EPS
            }],
            analyst_estimates: None,
            current_metrics: CurrentMetrics {
                price: 50.0,
                pe_ratio: -1.0,             // N/A for negative earnings
                shares_outstanding: 1000.0, // 1B shares
                name: Some("Loss Maker Inc".to_string()),
                exchange: Some("NASDAQ".to_string()),
                market_cap: Some("50B".to_string()),
                dividend_yield: None,
            },
        };

        let assumptions = ProjectionAssumptions {
            years: 3,
            bear_revenue_growth: 10.0,
            base_revenue_growth: 20.0,
            bull_revenue_growth: 30.0,
            bear_margin_change: -1.0, // Margins worsen
            base_margin_change: 2.0,  // Margins improve
            bull_margin_change: 5.0,  // Margins improve rapidly
            pe_low: 40.0,
            pe_high: 60.0,
            ps_low: 3.0,
            ps_high: 8.0,
            shares_growth: 0.0,
        };

        let projections = ProjectionService::generate_projections(&fundamental, &assumptions)
            .expect("mock fundamental fixture must produce projections");

        // Check that bear case uses P/S (negative EPS)
        let bear_first = &projections.bear[0];
        assert!(bear_first.eps < 0.0, "Bear case should have negative EPS");
        assert_eq!(
            bear_first.valuation_method, "P/S",
            "Should use P/S valuation"
        );
        assert!(
            bear_first.ps_low_est.is_some(),
            "Should have P/S low estimate"
        );
        assert!(
            bear_first.ps_high_est.is_some(),
            "Should have P/S high estimate"
        );
        assert!(
            bear_first.share_price_low > 0.0,
            "Share price should be positive"
        );
        assert!(
            bear_first.share_price_high > 0.0,
            "Share price should be positive"
        );

        // Check that bull case might transition to P/E (positive EPS if margins improve enough)
        let bull_last = &projections.bull[projections.bull.len() - 1];
        if bull_last.eps > 0.0 {
            assert_eq!(
                bull_last.valuation_method, "P/E",
                "Should use P/E valuation for positive EPS"
            );
            assert!(
                bull_last.ps_low_est.is_none(),
                "Should not have P/S estimates"
            );
        } else {
            assert_eq!(
                bull_last.valuation_method, "P/S",
                "Should use P/S valuation for negative EPS"
            );
            assert!(bull_last.ps_low_est.is_some(), "Should have P/S estimates");
        }

        println!("✓ Negative EPS correctly uses P/S valuation");
        println!(
            "  Bear EPS: {:.2}, Price: ${:.2}-${:.2} ({})",
            bear_first.eps,
            bear_first.share_price_low,
            bear_first.share_price_high,
            bear_first.valuation_method
        );
        println!(
            "  Bull EPS: {:.2}, Price: ${:.2}-${:.2} ({})",
            bull_last.eps,
            bull_last.share_price_low,
            bull_last.share_price_high,
            bull_last.valuation_method
        );
    }
}
