import type { FundamentalData, ProjectionResults } from "../../../shared/types/analysis";
import type { TickerAnalysisData } from "../../../shared/api/googleSheets";

/**
 * Convert frontend analysis data to backend TickerAnalysisData format for export
 */
export function convertToTickerAnalysisData(
  fundamentalData: FundamentalData,
  results: ProjectionResults
): TickerAnalysisData {
  const { symbol, historical, currentMetrics } = fundamentalData;
  const { baseline, projections, cagr } = results;

  // Get the last projection year for each scenario (final year)
  const lastYearProj = projections[projections.length - 1];
  const lastBase = lastYearProj.base;
  const lastBear = lastYearProj.bear;
  const lastBull = lastYearProj.bull;

  // Calculate upside percentage based on current price and target price
  const calculateUpside = (targetPrice: number) => {
    return ((targetPrice - currentMetrics.price) / currentMetrics.price) * 100;
  };

  return {
    ticker: symbol,
    company_name: currentMetrics.name || symbol,
    sector: null, // Could be added to FundamentalData if available
    market_cap: currentMetrics.marketCap || null,
    current_price: currentMetrics.price,
    pe_ratio: currentMetrics.peRatio,
    eps: historical[historical.length - 1]?.eps || null,
    historical_financials: historical.map((h, index) => ({
      year: h.year.toString(),
      revenue: h.revenue * 1_000_000_000, // Convert billions to dollars
      net_income: h.netIncome * 1_000_000_000, // Convert billions to dollars
      eps: h.eps,
      growth_rate: index > 0
        ? ((h.revenue - historical[index - 1].revenue) / historical[index - 1].revenue) * 100
        : null,
    })),
    projections: {
      base: {
        target_price: (lastBase.sharePriceLow + lastBase.sharePriceHigh) / 2,
        upside_percent: calculateUpside((lastBase.sharePriceLow + lastBase.sharePriceHigh) / 2),
        revenue_projection: lastBase.revenue * 1_000_000_000, // Convert billions to dollars
        eps_projection: lastBase.eps,
        timeline: `${lastBase.year}`,
      },
      bear: {
        target_price: (lastBear.sharePriceLow + lastBear.sharePriceHigh) / 2,
        upside_percent: calculateUpside((lastBear.sharePriceLow + lastBear.sharePriceHigh) / 2),
        revenue_projection: lastBear.revenue * 1_000_000_000,
        eps_projection: lastBear.eps,
        timeline: `${lastBear.year}`,
      },
      bull: {
        target_price: (lastBull.sharePriceLow + lastBull.sharePriceHigh) / 2,
        upside_percent: calculateUpside((lastBull.sharePriceLow + lastBull.sharePriceHigh) / 2),
        revenue_projection: lastBull.revenue * 1_000_000_000,
        eps_projection: lastBull.eps,
        timeline: `${lastBull.year}`,
      },
    },
    yearly_projections: projections.map(yearProj => ({
      year: yearProj.year,
      bear: {
        revenue: yearProj.bear.revenue * 1_000_000_000,
        net_income: yearProj.bear.netIncome * 1_000_000_000,
        eps: yearProj.bear.eps,
        share_price_low: yearProj.bear.sharePriceLow,
        share_price_high: yearProj.bear.sharePriceHigh,
      },
      base: {
        revenue: yearProj.base.revenue * 1_000_000_000,
        net_income: yearProj.base.netIncome * 1_000_000_000,
        eps: yearProj.base.eps,
        share_price_low: yearProj.base.sharePriceLow,
        share_price_high: yearProj.base.sharePriceHigh,
      },
      bull: {
        revenue: yearProj.bull.revenue * 1_000_000_000,
        net_income: yearProj.bull.netIncome * 1_000_000_000,
        eps: yearProj.bull.eps,
        share_price_low: yearProj.bull.sharePriceLow,
        share_price_high: yearProj.bull.sharePriceHigh,
      },
    })),
    baseline_year: baseline.year,
  };
}
