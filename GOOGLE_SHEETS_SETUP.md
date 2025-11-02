# Google Sheets Export Setup Guide

This guide explains how to configure Google Sheets OAuth2 credentials to enable exporting analysis data from Quantum Kapital to Google Sheets.

## Overview

The Google Sheets integration allows you to:
- Export ticker analysis data with one click
- Create a structured spreadsheet with dashboard and ticker-specific sheets
- Auto-populate company overview, historical financials, and forward projections
- Update dashboards with current portfolio data
- Access exported data from anywhere via Google Sheets

## Prerequisites

- A Google account
- Google Cloud Console access

## Step 1: Create a Google Cloud Project

1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Click on the project dropdown at the top of the page
3. Click "New Project"
4. Enter a project name (e.g., "Quantum Kapital")
5. Click "Create"

## Step 2: Enable Google Sheets API

1. In the Google Cloud Console, ensure your new project is selected
2. Navigate to "APIs & Services" > "Library"
3. Search for "Google Sheets API"
4. Click on "Google Sheets API" in the results
5. Click the "Enable" button

## Step 3: Configure OAuth Consent Screen

1. Navigate to "APIs & Services" > "OAuth consent screen"
2. Select "External" user type (unless you have a Google Workspace)
3. Click "Create"
4. Fill in the required fields:
   - **App name**: Quantum Kapital
   - **User support email**: Your email
   - **Developer contact information**: Your email
5. Click "Save and Continue"
6. On the "Scopes" page, click "Add or Remove Scopes"
7. Add the Google Sheets API scope:
   - Filter for "Google Sheets API"
   - Select `https://www.googleapis.com/auth/spreadsheets`
8. Click "Update" then "Save and Continue"
9. On "Test users", add your Google account email as a test user
10. Click "Save and Continue"

## Step 4: Create OAuth2 Credentials

1. Navigate to "APIs & Services" > "Credentials"
2. Click "Create Credentials" > "OAuth client ID"
3. Select "Desktop app" as the application type
4. Enter a name (e.g., "Quantum Kapital Desktop")
5. Click "Create"
6. A dialog will appear with your credentials
7. Click "Download JSON" to download the credentials file
8. Save the file securely

## Step 5: Configure Quantum Kapital

1. Open the downloaded JSON file in a text editor
2. Copy the entire JSON content
3. In Quantum Kapital:
   - Navigate to the Analysis tab
   - Search for and select a ticker
   - Click "Export to Google Sheets"
   - In the dialog, click "Configure Credentials"
   - Paste the JSON content into the text area
   - Click "Save Credentials"

## Step 6: First-Time Authentication

1. Click "Export" in the Google Sheets Export dialog
2. A browser window will open asking you to sign in to Google
3. Select your Google account
4. You may see a warning "Google hasn't verified this app"
   - Click "Advanced"
   - Click "Go to Quantum Kapital (unsafe)"
   - This is safe - it's your own application
5. Click "Allow" to grant permissions
6. The browser will redirect and you can close the window
7. Return to Quantum Kapital - the export will complete

## Step 7: Using the Export Feature

Once configured, you can:

### Export Individual Ticker Analysis
1. Navigate to Analysis tab
2. Search for and select a ticker
3. Wait for projections to load
4. Click "Export to Google Sheets" button
5. Click "Export" in the dialog
6. The export will create or update a sheet for that ticker
7. Click "Open Spreadsheet" to view in Google Sheets

### Spreadsheet Structure
The exported spreadsheet includes:

**Dashboard Sheet:**
- Ticker input field
- Total positions and portfolio value
- List of analyzed tickers with navigation links
- Last updated timestamp

**Ticker Sub-Sheets:** (one per ticker)
- Company Overview: Ticker, name, sector, market cap, P/E ratio, EPS
- Historical Financials: Year-over-year revenue, net income, EPS, growth rates
- Forward Projections:
  - Base Case scenario
  - Bear Case scenario
  - Bull Case scenario
  - Each with target price, upside %, revenue projection, and EPS projection

## Troubleshooting

### "Not authenticated" Error
- Re-run the authentication flow (Step 6)
- Check that your credentials JSON was saved correctly

### "Failed to create spreadsheet" Error
- Verify Google Sheets API is enabled in your Cloud Console
- Check that you granted the spreadsheets scope permission

### Browser Doesn't Open During Authentication
- The OAuth2 flow uses HTTP redirect on localhost
- Check your firewall/antivirus isn't blocking local connections
- Try disabling VPN temporarily

### "This app isn't verified" Warning
- This is normal for personal apps
- Follow the "Advanced" > "Go to app (unsafe)" steps
- This is safe as it's your own OAuth2 credentials

## Security Notes

- **Credentials Storage**: OAuth2 credentials and tokens are stored locally in:
  - macOS: `~/Library/Application Support/quantum-kapital/`
  - Linux: `~/.config/quantum-kapital/`
  - Windows: `%APPDATA%\quantum-kapital\`

- **Token Refresh**: Access tokens are automatically refreshed by the OAuth2 library

- **Permissions**: The app only requests `spreadsheets` scope - it can only access Google Sheets, not other Google services

- **Revoke Access**: You can revoke access anytime at https://myaccount.google.com/permissions

## Support

For issues or questions:
- Check the [GitHub Issues](https://github.com/frhd/quantum-kapital/issues)
- Refer to [Google OAuth2 Documentation](https://developers.google.com/identity/protocols/oauth2)
