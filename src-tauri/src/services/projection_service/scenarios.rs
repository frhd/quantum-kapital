use crate::ibkr::types::{
    CagrMetrics, FinancialProjection, FundamentalData, ProjectionAssumptions,
};

/// The three projected scenarios produced from one fundamental input.
pub(super) struct ScenarioBatch {
    pub(super) bear: Vec<FinancialProjection>,
    pub(super) base: Vec<FinancialProjection>,
    pub(super) bull: Vec<FinancialProjection>,
}

/// Run the bear/base/bull projection trio against shared inputs.
///
/// Both `generate_projections` and `generate_projection_results` take the
/// same baseline + assumptions and only differ in how they shape the
/// output. This helper centralizes the scenario configuration so the two
/// public methods stay in sync.
pub(super) fn generate_three_scenarios(
    fundamental: &FundamentalData,
    assumptions: &ProjectionAssumptions,
    initial_revenue: f64,
    initial_net_income: f64,
    projection_start_year: u32,
) -> ScenarioBatch {
    let scenario = |revenue_growth: f64, margin_change: f64| {
        generate_scenario_projection(ScenarioParams {
            initial_revenue,
            initial_net_income,
            initial_shares: fundamental.current_metrics.shares_outstanding,
            revenue_growth_rate: revenue_growth,
            margin_change_rate: margin_change,
            pe_low: assumptions.pe_low,
            pe_high: assumptions.pe_high,
            ps_low: assumptions.ps_low,
            ps_high: assumptions.ps_high,
            shares_growth_rate: assumptions.shares_growth,
            start_year: projection_start_year,
            num_years: assumptions.years,
            analyst_estimates: fundamental.analyst_estimates.as_ref(),
        })
    };

    ScenarioBatch {
        bear: scenario(
            assumptions.bear_revenue_growth,
            assumptions.bear_margin_change,
        ),
        base: scenario(
            assumptions.base_revenue_growth,
            assumptions.base_margin_change,
        ),
        bull: scenario(
            assumptions.bull_revenue_growth,
            assumptions.bull_margin_change,
        ),
    }
}

/// Parameters for generating projections for a single scenario
pub(super) struct ScenarioParams<'a> {
    pub(super) initial_revenue: f64,
    pub(super) initial_net_income: f64,
    pub(super) initial_shares: f64,
    pub(super) revenue_growth_rate: f64,
    pub(super) margin_change_rate: f64,
    pub(super) pe_low: f64,
    pub(super) pe_high: f64,
    pub(super) ps_low: f64,  // Price-to-Sales low (for negative EPS)
    pub(super) ps_high: f64, // Price-to-Sales high (for negative EPS)
    pub(super) shares_growth_rate: f64,
    pub(super) start_year: u32,
    pub(super) num_years: u32,
    pub(super) analyst_estimates: Option<&'a crate::ibkr::types::AnalystEstimates>,
}

fn generate_scenario_projection(params: ScenarioParams<'_>) -> Vec<FinancialProjection> {
    let mut projections = Vec::new();
    let mut revenue = params.initial_revenue;
    let mut shares = params.initial_shares;
    let mut margin = (params.initial_net_income / params.initial_revenue) * 100.0; // Calculate initial margin

    // Track previous net income for growth calculation (starts at baseline)
    let mut prev_net_income = params.initial_net_income;

    for year_offset in 0..params.num_years {
        let year = params.start_year + year_offset;

        // For the first projection year, check if we have analyst forward estimates
        // If available, use them as baseline instead of growing from historical data
        // This ensures projections reflect what the market is already pricing in
        let net_income = if year_offset == 0 {
            if let Some(estimates) = params.analyst_estimates {
                // Try to get analyst revenue estimate for this year
                revenue = estimates
                    .revenue
                    .iter()
                    .find(|e| e.year == year)
                    .map(|e| e.estimate)
                    .unwrap_or_else(|| revenue * (1.0 + params.revenue_growth_rate / 100.0));

                // Try to get analyst EPS estimate and back-calculate net income
                if let Some(eps_est) = estimates.eps.iter().find(|e| e.year == year) {
                    // Back-calculate net income from analyst EPS estimate
                    // EPS = (net_income / shares) * 1000, so net_income = EPS * shares / 1000
                    let net_income = eps_est.estimate * shares / 1_000.0;
                    margin = (net_income / revenue) * 100.0;
                    net_income
                } else {
                    // No analyst EPS, calculate from revenue and margin
                    margin += params.margin_change_rate;
                    revenue * (margin / 100.0)
                }
            } else {
                // No analyst estimates, apply growth rates to historical baseline
                revenue *= 1.0 + (params.revenue_growth_rate / 100.0);
                margin += params.margin_change_rate;
                revenue * (margin / 100.0)
            }
        } else {
            // For subsequent years (year_offset > 0), always compound growth from year 1
            revenue *= 1.0 + (params.revenue_growth_rate / 100.0);
            margin += params.margin_change_rate;
            revenue * (margin / 100.0)
        };

        shares *= 1.0 + (params.shares_growth_rate / 100.0);
        let eps = net_income / shares * 1_000.0; // Convert from billions and millions to per share

        // Hybrid valuation: Use P/E for positive EPS, P/S for negative EPS
        let (share_price_low, share_price_high, valuation_method, ps_low_est, ps_high_est) =
            if eps < 0.0 {
                // Company is losing money - use Price-to-Sales (P/S) valuation
                // P/S = Market Cap / Revenue
                // Share Price = (Revenue / Shares) × P/S Multiple
                let revenue_per_share = revenue / shares * 1_000.0; // Convert from billions and millions to per share
                (
                    revenue_per_share * params.ps_low,
                    revenue_per_share * params.ps_high,
                    "P/S".to_string(),
                    Some(params.ps_low),
                    Some(params.ps_high),
                )
            } else {
                // Company is profitable - use P/E valuation
                // Share Price = EPS × P/E Multiple
                (
                    eps * params.pe_low,
                    eps * params.pe_high,
                    "P/E".to_string(),
                    None,
                    None,
                )
            };

        // Calculate growth vs previous year (or baseline for first projection year)
        let net_income_growth = Some(((net_income - prev_net_income) / prev_net_income) * 100.0);

        // Find analyst EPS estimate for this year if available
        let analyst_eps_estimate = params.analyst_estimates.and_then(|estimates| {
            estimates
                .eps
                .iter()
                .find(|e| e.year == year)
                .map(|e| e.estimate)
        });

        projections.push(FinancialProjection {
            year,
            revenue,
            revenue_growth: params.revenue_growth_rate,
            net_income,
            net_income_growth,
            net_income_margins: margin,
            eps,
            pe_low_est: params.pe_low,
            pe_high_est: params.pe_high,
            share_price_low,
            share_price_high,
            valuation_method,
            ps_low_est,
            ps_high_est,
            analyst_eps_estimate,
        });

        prev_net_income = net_income;
    }

    projections
}

/// Calculate CAGR metrics for a projection scenario
pub(super) fn calculate_cagr(projections: &[FinancialProjection]) -> CagrMetrics {
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
