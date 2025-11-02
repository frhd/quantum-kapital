use super::auth::SheetsAuthenticator;
use super::types::*;
use google_sheets4::api::{
    BatchUpdateSpreadsheetRequest, CellData, CellFormat, Color, GridProperties, Request, Sheet,
    SheetProperties, Spreadsheet, SpreadsheetProperties, TextFormat, ValueRange,
};
use google_sheets4::hyper::client::HttpConnector;
use google_sheets4::hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use google_sheets4::{FieldMask, Sheets};

/// Service for managing Google Sheets exports
pub struct GoogleSheetsService {
    authenticator: SheetsAuthenticator,
    hub: Option<Sheets<HttpsConnector<HttpConnector>>>,
}

impl GoogleSheetsService {
    /// Create a new Google Sheets service
    pub fn new(authenticator: SheetsAuthenticator) -> Self {
        Self {
            authenticator,
            hub: None,
        }
    }

    /// Initialize the Google Sheets API hub
    pub async fn initialize(&mut self) -> Result<(), SheetsError> {
        let auth = self.authenticator.authenticate().await?;

        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .map_err(|e| SheetsError::ApiError(format!("Failed to build HTTPS connector: {e}")))?
            .https_only()
            .enable_http1()
            .build();

        let client = google_sheets4::hyper::Client::builder().build(https);

        let hub = Sheets::new(client, auth);
        self.hub = Some(hub);

        Ok(())
    }

    /// Get the hub, ensuring it's initialized
    fn get_hub(&self) -> Result<&Sheets<HttpsConnector<HttpConnector>>, SheetsError> {
        self.hub
            .as_ref()
            .ok_or_else(|| SheetsError::ApiError("Service not initialized".to_string()))
    }

    /// Create a new spreadsheet with dashboard and initial structure
    pub async fn create_spreadsheet(&self, name: &str) -> Result<String, SheetsError> {
        let hub = self.get_hub()?;

        // Create spreadsheet with dashboard sheet
        let spreadsheet = Spreadsheet {
            properties: Some(SpreadsheetProperties {
                title: Some(name.to_string()),
                ..Default::default()
            }),
            sheets: Some(vec![Sheet {
                properties: Some(SheetProperties {
                    title: Some("Dashboard".to_string()),
                    grid_properties: Some(GridProperties {
                        row_count: Some(100),
                        column_count: Some(10),
                        frozen_row_count: Some(1),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        };

        let result = hub
            .spreadsheets()
            .create(spreadsheet)
            .doit()
            .await
            .map_err(|e| SheetsError::GoogleError(format!("Failed to create spreadsheet: {e}")))?;

        let spreadsheet_id = result
            .1
            .spreadsheet_id
            .ok_or_else(|| SheetsError::ApiError("No spreadsheet ID returned".to_string()))?;

        Ok(spreadsheet_id)
    }

    /// Create the dashboard sheet with initial structure
    pub async fn setup_dashboard(
        &self,
        spreadsheet_id: &str,
        data: &DashboardData,
    ) -> Result<(), SheetsError> {
        let hub = self.get_hub()?;

        // Create header row
        let mut rows = vec![vec![
            "Quantum Kapital - Portfolio Analysis Dashboard".to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]];

        // Add empty row
        rows.push(vec![String::new(); 10]);

        // Add ticker input section with instructions
        rows.push(vec![
            "Click a ticker below to view its analysis →".to_string(),
            String::new(),
            String::new(),
            "Total Positions:".to_string(),
            data.total_positions.to_string(),
        ]);

        rows.push(vec![
            String::new(),
            String::new(),
            String::new(),
            "Total Value:".to_string(),
            format!("${:.2}", data.total_value),
        ]);

        // Add empty row
        rows.push(vec![String::new(); 10]);

        // Add analyzed tickers header
        rows.push(vec![
            "Ticker".to_string(),
            "Click to View".to_string(),
            "Last Updated".to_string(),
        ]);

        // Add ticker list with hyperlink formulas
        for ticker in &data.analyzed_tickers {
            rows.push(vec![
                ticker.clone(),
                // Create a hyperlink formula that links to the ticker sheet
                format!(
                    "=HYPERLINK(\"#gid=0&range={0}!A1\", \"→ View {0}\")",
                    ticker
                ),
                data.last_updated.clone(),
            ]);
        }

        // Convert to ValueRange
        let values: Vec<Vec<serde_json::Value>> = rows
            .into_iter()
            .map(|row| row.into_iter().map(serde_json::Value::String).collect())
            .collect();

        let value_range = ValueRange {
            range: Some("Dashboard!A1".to_string()),
            major_dimension: Some("ROWS".to_string()),
            values: Some(values),
        };

        hub.spreadsheets()
            .values_update(value_range, spreadsheet_id, "Dashboard!A1")
            .value_input_option("USER_ENTERED") // Parse formulas
            .doit()
            .await
            .map_err(|e| SheetsError::GoogleError(format!("Failed to update dashboard: {e}")))?;

        // Format the dashboard
        self.format_dashboard(spreadsheet_id).await?;

        Ok(())
    }

    /// Format the dashboard sheet with colors and styles
    async fn format_dashboard(&self, spreadsheet_id: &str) -> Result<(), SheetsError> {
        let hub = self.get_hub()?;

        // Get the dashboard sheet ID (it's the first sheet, ID 0)
        let requests = vec![
            // Format header (row 1)
            Request {
                repeat_cell: Some(google_sheets4::api::RepeatCellRequest {
                    range: Some(google_sheets4::api::GridRange {
                        sheet_id: Some(0),
                        start_row_index: Some(0),
                        end_row_index: Some(1),
                        start_column_index: Some(0),
                        end_column_index: Some(10),
                    }),
                    cell: Some(CellData {
                        user_entered_format: Some(CellFormat {
                            background_color: Some(Color {
                                red: Some(0.2),
                                green: Some(0.4),
                                blue: Some(0.8),
                                alpha: Some(1.0),
                            }),
                            text_format: Some(TextFormat {
                                foreground_color: Some(Color {
                                    red: Some(1.0),
                                    green: Some(1.0),
                                    blue: Some(1.0),
                                    alpha: Some(1.0),
                                }),
                                font_size: Some(14),
                                bold: Some(true),
                                ..Default::default()
                            }),
                            horizontal_alignment: Some("CENTER".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    fields: Some(FieldMask::new(&[
                        "userEnteredFormat.backgroundColor",
                        "userEnteredFormat.textFormat",
                        "userEnteredFormat.horizontalAlignment",
                    ])),
                }),
                ..Default::default()
            },
        ];

        let batch_update = BatchUpdateSpreadsheetRequest {
            requests: Some(requests),
            ..Default::default()
        };

        hub.spreadsheets()
            .batch_update(batch_update, spreadsheet_id)
            .doit()
            .await
            .map_err(|e| SheetsError::GoogleError(format!("Failed to format dashboard: {e}")))?;

        Ok(())
    }

    /// Create a new sheet for a ticker's analysis
    pub async fn create_ticker_sheet(
        &self,
        spreadsheet_id: &str,
        ticker: &str,
    ) -> Result<i32, SheetsError> {
        let hub = self.get_hub()?;

        let requests = vec![Request {
            add_sheet: Some(google_sheets4::api::AddSheetRequest {
                properties: Some(SheetProperties {
                    title: Some(ticker.to_string()),
                    grid_properties: Some(GridProperties {
                        row_count: Some(100),
                        column_count: Some(8),
                        frozen_row_count: Some(1),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        }];

        let batch_update = BatchUpdateSpreadsheetRequest {
            requests: Some(requests),
            ..Default::default()
        };

        let result = hub
            .spreadsheets()
            .batch_update(batch_update, spreadsheet_id)
            .doit()
            .await
            .map_err(|e| SheetsError::GoogleError(format!("Failed to create ticker sheet: {e}")))?;

        // Extract the new sheet ID from the response
        let sheet_id = result
            .1
            .replies
            .and_then(|replies| replies.first().cloned())
            .and_then(|reply| reply.add_sheet)
            .and_then(|add_sheet| add_sheet.properties)
            .and_then(|props| props.sheet_id)
            .ok_or_else(|| SheetsError::ApiError("Failed to get new sheet ID".to_string()))?;

        Ok(sheet_id)
    }

    /// Populate a ticker sheet with analysis data
    pub async fn populate_ticker_sheet(
        &self,
        spreadsheet_id: &str,
        ticker: &str,
        data: &TickerAnalysisData,
    ) -> Result<(), SheetsError> {
        let hub = self.get_hub()?;

        let mut rows = vec![];

        // Section A: Company Overview
        rows.push(vec!["Company Overview".to_string()]);
        rows.push(vec![String::new()]);
        rows.push(vec!["Ticker:".to_string(), data.ticker.clone()]);
        rows.push(vec!["Company:".to_string(), data.company_name.clone()]);
        rows.push(vec![
            "Sector:".to_string(),
            data.sector.clone().unwrap_or_default(),
        ]);
        rows.push(vec![
            "Market Cap:".to_string(),
            data.market_cap.clone().unwrap_or_default(),
        ]);
        rows.push(vec![
            "Current Price:".to_string(),
            data.current_price
                .map(|p| format!("${p:.2}"))
                .unwrap_or_default(),
        ]);
        rows.push(vec![
            "P/E Ratio:".to_string(),
            data.pe_ratio.map(|p| format!("{p:.2}")).unwrap_or_default(),
        ]);
        rows.push(vec![
            "EPS:".to_string(),
            data.eps.map(|e| format!("${e:.2}")).unwrap_or_default(),
        ]);
        rows.push(vec![String::new()]);

        // Section B: Historical Financials
        rows.push(vec!["Historical Financials".to_string()]);
        rows.push(vec![
            "Year".to_string(),
            "Revenue".to_string(),
            "Net Income".to_string(),
            "EPS".to_string(),
            "Growth %".to_string(),
        ]);

        for hist in &data.historical_financials {
            rows.push(vec![
                hist.year.clone(),
                hist.revenue.map(|r| format!("${r:.0}")).unwrap_or_default(),
                hist.net_income
                    .map(|n| format!("${n:.0}"))
                    .unwrap_or_default(),
                hist.eps.map(|e| format!("${e:.2}")).unwrap_or_default(),
                hist.growth_rate
                    .map(|g| format!("{g:.1}%"))
                    .unwrap_or_default(),
            ]);
        }

        rows.push(vec![String::new()]);

        // Section C: Projections
        rows.push(vec!["Forward Projections".to_string()]);
        rows.push(vec![
            "Scenario".to_string(),
            "Target Price".to_string(),
            "Upside %".to_string(),
            "Revenue Proj".to_string(),
            "EPS Proj".to_string(),
            "Timeline".to_string(),
        ]);

        // Base scenario
        rows.push(vec![
            "Base".to_string(),
            format!("${:.2}", data.projections.base.target_price),
            format!("{:.1}%", data.projections.base.upside_percent),
            format!("${:.0}", data.projections.base.revenue_projection),
            format!("${:.2}", data.projections.base.eps_projection),
            data.projections.base.timeline.clone(),
        ]);

        // Bear scenario
        rows.push(vec![
            "Bear".to_string(),
            format!("${:.2}", data.projections.bear.target_price),
            format!("{:.1}%", data.projections.bear.upside_percent),
            format!("${:.0}", data.projections.bear.revenue_projection),
            format!("${:.2}", data.projections.bear.eps_projection),
            data.projections.bear.timeline.clone(),
        ]);

        // Bull scenario
        rows.push(vec![
            "Bull".to_string(),
            format!("${:.2}", data.projections.bull.target_price),
            format!("{:.1}%", data.projections.bull.upside_percent),
            format!("${:.0}", data.projections.bull.revenue_projection),
            format!("${:.2}", data.projections.bull.eps_projection),
            data.projections.bull.timeline.clone(),
        ]);

        // Convert to ValueRange
        let values: Vec<Vec<serde_json::Value>> = rows
            .into_iter()
            .map(|row| row.into_iter().map(serde_json::Value::String).collect())
            .collect();

        let range = format!("{ticker}!A1");
        let value_range = ValueRange {
            range: Some(range.clone()),
            major_dimension: Some("ROWS".to_string()),
            values: Some(values),
        };

        hub.spreadsheets()
            .values_update(value_range, spreadsheet_id, &range)
            .value_input_option("RAW")
            .doit()
            .await
            .map_err(|e| {
                SheetsError::GoogleError(format!("Failed to populate ticker sheet: {e}"))
            })?;

        Ok(())
    }

    /// Get the URL for a spreadsheet
    pub fn get_spreadsheet_url(&self, spreadsheet_id: &str) -> String {
        format!("https://docs.google.com/spreadsheets/d/{spreadsheet_id}/edit")
    }
}
