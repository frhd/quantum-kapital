use super::auth::SheetsAuthenticator;
use super::types::*;
use google_sheets4::api::{
    BatchUpdateSpreadsheetRequest, Border, CellData, CellFormat, Color, GridProperties, GridRange,
    RepeatCellRequest, Request, Sheet, SheetProperties, Spreadsheet, SpreadsheetProperties,
    TextFormat, UpdateBordersRequest, ValueRange,
};
use google_sheets4::hyper::client::HttpConnector;
use google_sheets4::hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use google_sheets4::{FieldMask, Sheets};

/// Format a number with K/M/B suffix
fn format_number(value: f64) -> String {
    let abs_value = value.abs();
    let sign = if value < 0.0 { "-" } else { "" };

    if abs_value >= 1_000_000_000.0 {
        format!("{}${:.2}B", sign, abs_value / 1_000_000_000.0)
    } else if abs_value >= 1_000_000.0 {
        format!("{}${:.2}M", sign, abs_value / 1_000_000.0)
    } else if abs_value >= 1_000.0 {
        format!("{}${:.2}K", sign, abs_value / 1_000.0)
    } else {
        format!("{sign}${abs_value:.2}")
    }
}

/// Apply formatting to a ticker sheet
async fn apply_sheet_formatting(
    hub: &Sheets<HttpsConnector<HttpConnector>>,
    spreadsheet_id: &str,
    sheet_id: i32,
    data: &TickerAnalysisData,
) -> Result<(), SheetsError> {
    let mut requests = Vec::new();

    // Calculate row positions based on data structure
    let mut current_row: i32 = 0;

    // Section A: Company Overview (1 header + 1 blank + 7 data rows + 1 blank = 10 rows)
    let overview_header_row = current_row;
    current_row += 10;

    // Section B: Historical Financials (1 header + 1 table header + N data rows + 1 blank)
    let historical_header_row = current_row;
    current_row += 1; // Header "Historical Financials"
    let historical_table_header = current_row;
    current_row += 1; // Table header row
    current_row += data.historical_financials.len() as i32; // Historical data rows
    current_row += 1; // Blank row

    // Section C: Year-by-Year Projections (if available)
    let projections_start = current_row;
    let mut baseline_row: Option<i32> = None;
    let mut year_rows: Vec<i32> = Vec::new();

    if data.yearly_projections.is_some() {
        current_row += 1; // "Year-by-Year Projections" header
        let _table_header = current_row;
        current_row += 1; // Table header

        if data.baseline_year.is_some() {
            baseline_row = Some(current_row);
            current_row += 1; // Baseline row
        }

        // Track the start row of each year (for bear scenario)
        if let Some(yearly) = &data.yearly_projections {
            for _ in 0..yearly.len() {
                year_rows.push(current_row); // Bear row for this year
                current_row += 3; // Bear + Base + Bull
            }
        }
        current_row += 1; // Blank row
    }

    // Section D: Final Year Targets starts here
    let targets_header_row = current_row;

    // 1. Format section headers with light blue background and bold text
    let section_headers = vec![
        overview_header_row,
        historical_header_row,
        projections_start,
        targets_header_row,
    ];

    for header_row in section_headers {
        requests.push(Request {
            repeat_cell: Some(RepeatCellRequest {
                range: Some(GridRange {
                    sheet_id: Some(sheet_id),
                    start_row_index: Some(header_row),
                    end_row_index: Some(header_row + 1),
                    start_column_index: Some(0),
                    end_column_index: Some(6),
                }),
                cell: Some(CellData {
                    user_entered_format: Some(CellFormat {
                        background_color: Some(Color {
                            red: Some(0.85),
                            green: Some(0.92),
                            blue: Some(0.97),
                            alpha: None,
                        }),
                        text_format: Some(TextFormat {
                            bold: Some(true),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                fields: Some(FieldMask::new(&[
                    "userEnteredFormat.backgroundColor",
                    "userEnteredFormat.textFormat",
                ])),
            }),
            ..Default::default()
        });
    }

    // 2. Format table headers (Historical and Projections) with gray background
    let table_headers = vec![historical_table_header];
    let projections_table_header = if data.yearly_projections.is_some() {
        Some(projections_start + 1)
    } else {
        None
    };

    for header_row in table_headers
        .into_iter()
        .chain(projections_table_header.into_iter())
    {
        requests.push(Request {
            repeat_cell: Some(RepeatCellRequest {
                range: Some(GridRange {
                    sheet_id: Some(sheet_id),
                    start_row_index: Some(header_row),
                    end_row_index: Some(header_row + 1),
                    start_column_index: Some(0),
                    end_column_index: Some(6),
                }),
                cell: Some(CellData {
                    user_entered_format: Some(CellFormat {
                        background_color: Some(Color {
                            red: Some(0.9),
                            green: Some(0.9),
                            blue: Some(0.9),
                            alpha: None,
                        }),
                        text_format: Some(TextFormat {
                            bold: Some(true),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                fields: Some(FieldMask::new(&[
                    "userEnteredFormat.backgroundColor",
                    "userEnteredFormat.textFormat",
                ])),
            }),
            ..Default::default()
        });
    }

    // 3. Format baseline row with light green background
    if let Some(baseline) = baseline_row {
        requests.push(Request {
            repeat_cell: Some(RepeatCellRequest {
                range: Some(GridRange {
                    sheet_id: Some(sheet_id),
                    start_row_index: Some(baseline),
                    end_row_index: Some(baseline + 1),
                    start_column_index: Some(0),
                    end_column_index: Some(6),
                }),
                cell: Some(CellData {
                    user_entered_format: Some(CellFormat {
                        background_color: Some(Color {
                            red: Some(0.9),
                            green: Some(0.95),
                            blue: Some(0.9),
                            alpha: None,
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                fields: Some(FieldMask::new(&["userEnteredFormat.backgroundColor"])),
            }),
            ..Default::default()
        });
    }

    // 4. Format scenario rows with subtle colors
    // Bear: light red, Base: light yellow, Bull: light green
    for year_start in &year_rows {
        // Bear (light red)
        requests.push(Request {
            repeat_cell: Some(RepeatCellRequest {
                range: Some(GridRange {
                    sheet_id: Some(sheet_id),
                    start_row_index: Some(*year_start),
                    end_row_index: Some(*year_start + 1),
                    start_column_index: Some(0),
                    end_column_index: Some(6),
                }),
                cell: Some(CellData {
                    user_entered_format: Some(CellFormat {
                        background_color: Some(Color {
                            red: Some(1.0),
                            green: Some(0.95),
                            blue: Some(0.95),
                            alpha: None,
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                fields: Some(FieldMask::new(&["userEnteredFormat.backgroundColor"])),
            }),
            ..Default::default()
        });

        // Base (light yellow)
        requests.push(Request {
            repeat_cell: Some(RepeatCellRequest {
                range: Some(GridRange {
                    sheet_id: Some(sheet_id),
                    start_row_index: Some(*year_start + 1),
                    end_row_index: Some(*year_start + 2),
                    start_column_index: Some(0),
                    end_column_index: Some(6),
                }),
                cell: Some(CellData {
                    user_entered_format: Some(CellFormat {
                        background_color: Some(Color {
                            red: Some(1.0),
                            green: Some(1.0),
                            blue: Some(0.95),
                            alpha: None,
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                fields: Some(FieldMask::new(&["userEnteredFormat.backgroundColor"])),
            }),
            ..Default::default()
        });

        // Bull (light green)
        requests.push(Request {
            repeat_cell: Some(RepeatCellRequest {
                range: Some(GridRange {
                    sheet_id: Some(sheet_id),
                    start_row_index: Some(*year_start + 2),
                    end_row_index: Some(*year_start + 3),
                    start_column_index: Some(0),
                    end_column_index: Some(6),
                }),
                cell: Some(CellData {
                    user_entered_format: Some(CellFormat {
                        background_color: Some(Color {
                            red: Some(0.95),
                            green: Some(1.0),
                            blue: Some(0.95),
                            alpha: None,
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                fields: Some(FieldMask::new(&["userEnteredFormat.backgroundColor"])),
            }),
            ..Default::default()
        });

        // Add border after bull row (between years)
        requests.push(Request {
            update_borders: Some(UpdateBordersRequest {
                range: Some(GridRange {
                    sheet_id: Some(sheet_id),
                    start_row_index: Some(*year_start + 2),
                    end_row_index: Some(*year_start + 3),
                    start_column_index: Some(0),
                    end_column_index: Some(6),
                }),
                bottom: Some(Border {
                    style: Some("SOLID".to_string()),
                    width: Some(2),
                    color: Some(Color {
                        red: Some(0.7),
                        green: Some(0.7),
                        blue: Some(0.7),
                        alpha: None,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
    }

    // 5. Right-align all numeric columns (columns B-F, which are index 1-5)
    // Apply to all data rows (skip the very first header but get everything else with numbers)
    requests.push(Request {
        repeat_cell: Some(RepeatCellRequest {
            range: Some(GridRange {
                sheet_id: Some(sheet_id),
                start_row_index: Some(2),    // Start after first header
                end_row_index: None,         // To the end
                start_column_index: Some(1), // Column B
                end_column_index: Some(6),   // Through column F
            }),
            cell: Some(CellData {
                user_entered_format: Some(CellFormat {
                    horizontal_alignment: Some("RIGHT".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            fields: Some(FieldMask::new(&["userEnteredFormat.horizontalAlignment"])),
        }),
        ..Default::default()
    });

    // Apply all formatting requests in a single batch
    let batch_request = BatchUpdateSpreadsheetRequest {
        requests: Some(requests),
        ..Default::default()
    };

    hub.spreadsheets()
        .batch_update(batch_request, spreadsheet_id)
        .doit()
        .await
        .map_err(|e| SheetsError::ApiError(format!("Failed to apply formatting: {e}")))?;

    Ok(())
}

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
                hist.revenue.map(format_number).unwrap_or_default(),
                hist.net_income.map(format_number).unwrap_or_default(),
                hist.eps.map(|e| format!("${e:.2}")).unwrap_or_default(),
                hist.growth_rate
                    .map(|g| format!("{g:.1}%"))
                    .unwrap_or_default(),
            ]);
        }

        rows.push(vec![String::new()]);

        // Section C: Year-by-Year Projections (if available)
        if let Some(yearly) = &data.yearly_projections {
            rows.push(vec!["Year-by-Year Projections".to_string()]);
            rows.push(vec![
                "Year".to_string(),
                "Scenario".to_string(),
                "Revenue".to_string(),
                "Net Income".to_string(),
                "EPS".to_string(),
                "Share Price Range".to_string(),
            ]);

            // Add baseline year if available
            if let Some(baseline_year) = data.baseline_year {
                rows.push(vec![
                    format!("{} (Baseline)", baseline_year),
                    "Actual".to_string(),
                    format!(
                        "${:.2}B",
                        data.historical_financials
                            .last()
                            .map(|h| h.revenue.unwrap_or(0.0) / 1_000_000_000.0)
                            .unwrap_or(0.0)
                    ),
                    format!(
                        "${:.2}B",
                        data.historical_financials
                            .last()
                            .map(|h| h.net_income.unwrap_or(0.0) / 1_000_000_000.0)
                            .unwrap_or(0.0)
                    ),
                    format!("${:.2}", data.eps.unwrap_or(0.0)),
                    format!("${:.2}", data.current_price.unwrap_or(0.0)),
                ]);
            }

            // Add each projection year
            for year_proj in yearly {
                // Bear scenario row
                rows.push(vec![
                    year_proj.year.to_string(),
                    "Bear".to_string(),
                    format!("${:.2}B", year_proj.bear.revenue / 1_000_000_000.0),
                    format!("${:.2}B", year_proj.bear.net_income / 1_000_000_000.0),
                    format!("${:.2}", year_proj.bear.eps),
                    format!(
                        "${:.2}-${:.2}",
                        year_proj.bear.share_price_low, year_proj.bear.share_price_high
                    ),
                ]);

                // Base scenario row
                rows.push(vec![
                    String::new(),
                    "Base".to_string(),
                    format!("${:.2}B", year_proj.base.revenue / 1_000_000_000.0),
                    format!("${:.2}B", year_proj.base.net_income / 1_000_000_000.0),
                    format!("${:.2}", year_proj.base.eps),
                    format!(
                        "${:.2}-${:.2}",
                        year_proj.base.share_price_low, year_proj.base.share_price_high
                    ),
                ]);

                // Bull scenario row
                rows.push(vec![
                    String::new(),
                    "Bull".to_string(),
                    format!("${:.2}B", year_proj.bull.revenue / 1_000_000_000.0),
                    format!("${:.2}B", year_proj.bull.net_income / 1_000_000_000.0),
                    format!("${:.2}", year_proj.bull.eps),
                    format!(
                        "${:.2}-${:.2}",
                        year_proj.bull.share_price_low, year_proj.bull.share_price_high
                    ),
                ]);
            }

            rows.push(vec![String::new()]);
        }

        // Section D: Final Year Targets Summary
        rows.push(vec!["Final Year Target Summary".to_string()]);
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
            format_number(data.projections.base.revenue_projection),
            format!("${:.2}", data.projections.base.eps_projection),
            data.projections.base.timeline.clone(),
        ]);

        // Bear scenario
        rows.push(vec![
            "Bear".to_string(),
            format!("${:.2}", data.projections.bear.target_price),
            format!("{:.1}%", data.projections.bear.upside_percent),
            format_number(data.projections.bear.revenue_projection),
            format!("${:.2}", data.projections.bear.eps_projection),
            data.projections.bear.timeline.clone(),
        ]);

        // Bull scenario
        rows.push(vec![
            "Bull".to_string(),
            format!("${:.2}", data.projections.bull.target_price),
            format!("{:.1}%", data.projections.bull.upside_percent),
            format_number(data.projections.bull.revenue_projection),
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

        // Get the sheet_id for the ticker sheet to apply formatting
        let spreadsheet = hub
            .spreadsheets()
            .get(spreadsheet_id)
            .doit()
            .await
            .map_err(|e| {
                SheetsError::GoogleError(format!("Failed to get spreadsheet metadata: {e}"))
            })?
            .1;

        let sheet_id = spreadsheet
            .sheets
            .and_then(|sheets| {
                sheets.into_iter().find_map(|sheet| {
                    sheet.properties.and_then(|props| {
                        if props.title.as_deref() == Some(ticker) {
                            props.sheet_id
                        } else {
                            None
                        }
                    })
                })
            })
            .ok_or_else(|| {
                SheetsError::ApiError(format!("Could not find sheet for ticker {ticker}"))
            })?;

        // Apply formatting to the sheet
        apply_sheet_formatting(hub, spreadsheet_id, sheet_id, data).await?;

        Ok(())
    }

    /// Get the URL for a spreadsheet
    pub fn get_spreadsheet_url(&self, spreadsheet_id: &str) -> String {
        format!("https://docs.google.com/spreadsheets/d/{spreadsheet_id}/edit")
    }
}
