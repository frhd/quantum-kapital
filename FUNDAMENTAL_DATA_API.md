# Fundamental Data API Integration

This document explains how to set up and use the external API integration for fetching real fundamental data for financial projections.

## Overview

The application uses **Financial Modeling Prep (FMP) API** to fetch real fundamental data including:
- Historical income statements (revenue, net income, EPS)
- Current stock price and P/E ratio
- Shares outstanding
- Analyst estimates for future revenue and EPS

This data powers the forward-looking analysis feature with Bear/Base/Bull scenario projections.

## Getting an API Key

### 1. Sign up for Financial Modeling Prep

1. Visit https://financialmodelingprep.com/developer/docs/
2. Click "Get Your Free API Key"
3. Sign up for a free account
4. Navigate to your dashboard to find your API key

### 2. Free Tier Limitations

- **250 API calls per day**
- Access to most fundamental data endpoints
- No credit card required
- Perfect for development and testing

### 3. Upgrade Options (Optional)

If you need more calls:
- **Starter**: $14/month - 750 calls/day
- **Professional**: $49/month - Unlimited calls

## Setup Instructions

### Step 1: Copy the Environment File Template

```bash
cd src-tauri
cp .env.example .env
```

### Step 2: Add Your API Key

Edit `src-tauri/.env` and replace `your_api_key_here` with your actual API key:

```bash
FMP_API_KEY=abc123def456...
```

### Step 3: Verify the Setup

The application will automatically:
1. Load the `.env` file on startup
2. Try to fetch real data from FMP when you select a ticker
3. Fall back to mock data if:
   - API key is not set
   - API request fails
   - Rate limit is exceeded

## How It Works

### Architecture Flow

```
User selects ticker (e.g., AAPL)
    ↓
ibkr_get_fundamental_data command
    ↓
Check for FMP_API_KEY env var
    ↓
    ├─ If present: FinancialDataService
    │       ↓
    │   Fetch from FMP API (parallel requests)
    │   - Income statements
    │   - Company profile (price, market cap)
    │   - Key metrics (P/E ratio)
    │   - Analyst estimates
    │       ↓
    │   Parse and map to FundamentalData
    │       ↓
    │   ProjectionService generates scenarios
    │
    └─ If missing or error: Use mock data
            ↓
        ProjectionService generates scenarios
```

### Data Mapping

The FMP API responses are mapped to our `FundamentalData` structure:

```rust
FundamentalData {
    symbol: "AAPL",
    historical: Vec<HistoricalFinancial> {
        // Last 5 years from income statements
        { year: 2023, revenue: 383.29B, net_income: 96.99B, eps: 6.13 },
        { year: 2022, revenue: 394.33B, net_income: 99.80B, eps: 6.15 },
        ...
    },
    analyst_estimates: Some(AnalystEstimates {
        revenue: [{ year: 2024, estimate: 400.5B }, ...],
        eps: [{ year: 2024, estimate: 6.50 }, ...]
    }),
    current_metrics: CurrentMetrics {
        price: 178.50,
        pe_ratio: 29.12,
        shares_outstanding: 15634.0 // in millions
    }
}
```

## API Endpoints Used

### 1. Income Statements
```
GET /api/v3/income-statement/{symbol}?limit=5&apikey={key}
```
Returns: Historical revenue, net income, EPS

### 2. Company Profile
```
GET /api/v3/profile/{symbol}?apikey={key}
```
Returns: Current price, market cap

### 3. Key Metrics
```
GET /api/v3/key-metrics/{symbol}?limit=1&apikey={key}
```
Returns: P/E ratio, other valuation metrics

### 4. Analyst Estimates
```
GET /api/v3/analyst-estimates/{symbol}?limit=5&apikey={key}
```
Returns: Forward revenue and EPS estimates

## Troubleshooting

### Issue: "Failed to fetch real data" in logs

**Possible causes:**
1. **Invalid API key**: Verify your API key is correct
2. **Rate limit exceeded**: Free tier is limited to 250 calls/day
3. **Invalid ticker symbol**: Use correct stock symbols (e.g., "AAPL" not "Apple")
4. **Network error**: Check internet connection

**Solution:**
- Check Tauri logs for detailed error messages
- Verify `.env` file is in `src-tauri/` directory
- Ensure API key is valid by testing at https://financialmodelingprep.com/api/v3/profile/AAPL?apikey=YOUR_KEY

### Issue: Application still shows mock data

**Possible causes:**
1. `.env` file not created
2. API key not set in `.env`
3. Application not restarted after adding `.env`

**Solution:**
1. Ensure `src-tauri/.env` exists with `FMP_API_KEY=...`
2. Restart the application completely (quit and relaunch)
3. Check logs for "FMP_API_KEY not set" message

### Issue: Rate limit errors

**Solution:**
1. Upgrade to paid plan if needed
2. Implement caching (future feature)
3. Use mock data for development/testing

## Development Notes

### Mock Data Fallback

The application gracefully falls back to mock data when:
- FMP_API_KEY is not set (for development without API key)
- API requests fail (network errors, rate limits)
- API returns invalid data

This ensures the application always works, even without real data.

### Logging

Check the application logs for detailed information:
- `info!` level: Successful API fetches
- `warn!` level: Fallback to mock data with reason

Example logs:
```
[INFO] Fetching real fundamental data for AAPL from FMP API
[INFO] Successfully fetched real fundamental data for AAPL
```

Or:
```
[INFO] FMP_API_KEY not set. Using mock data for AAPL
```

## Testing

### Test with Popular Tickers

Try these tickers to verify the integration:
- **AAPL** - Apple Inc.
- **MSFT** - Microsoft
- **NVDA** - NVIDIA
- **GOOGL** - Alphabet (Google)
- **TSLA** - Tesla

### Verify Real Data

You'll know real data is being used when:
1. Revenue values match current financial reports
2. Current price matches live stock price
3. Historical data shows actual company financials

### Compare with Mock Data

Without API key, you'll see:
- Generic "mock" revenue patterns
- Unrealistic financial metrics
- Consistent data regardless of ticker

## Future Enhancements

Planned improvements:
1. **Response caching** - Reduce API calls by caching responses
2. **Alternative API providers** - Support for Alpha Vantage, IEX Cloud
3. **Offline mode** - Save fetched data for offline use
4. **Custom data import** - Manual CSV/JSON data upload
5. **Rate limit handling** - Automatic retry with exponential backoff

## Security Notes

⚠️ **Important Security Practices:**

1. **Never commit `.env` file** - Already added to `.gitignore`
2. **Don't share API keys** - Each user should use their own key
3. **Use environment variables** - Never hardcode API keys in source code
4. **Rotate keys regularly** - Generate new keys if compromised
5. **Monitor usage** - Check FMP dashboard for unexpected usage

## Support

For issues with:
- **FMP API**: Contact Financial Modeling Prep support
- **Application integration**: Create issue in project repository
- **General questions**: Check project documentation

## Additional Resources

- [FMP API Documentation](https://financialmodelingprep.com/developer/docs/)
- [FMP API Dashboard](https://financialmodelingprep.com/developer/account)
- [Tauri Environment Variables](https://tauri.app/v1/guides/building/app-config)
