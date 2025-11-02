import { invoke } from "@tauri-apps/api/core";

// Types matching Rust structures

export interface AuthState {
  authenticated: boolean;
  user_email: string | null;
}

export interface SheetsConfig {
  spreadsheet_id: string | null;
  spreadsheet_name: string;
}

export interface HistoricalFinancial {
  year: string;
  revenue: number | null;
  net_income: number | null;
  eps: number | null;
  growth_rate: number | null;
}

export interface ScenarioProjection {
  target_price: number;
  upside_percent: number;
  revenue_projection: number;
  eps_projection: number;
  timeline: string;
}

export interface ProjectionData {
  base: ScenarioProjection;
  bear: ScenarioProjection;
  bull: ScenarioProjection;
}

export interface TickerAnalysisData {
  ticker: string;
  company_name: string;
  sector: string | null;
  market_cap: string | null;
  current_price: number | null;
  pe_ratio: number | null;
  eps: number | null;
  historical_financials: HistoricalFinancial[];
  projections: ProjectionData;
}

export interface DashboardData {
  total_positions: number;
  total_value: number;
  analyzed_tickers: string[];
  last_updated: string;
}

export interface ExportResult {
  success: boolean;
  spreadsheet_id: string;
  spreadsheet_url: string;
  sheets_created: string[];
  message: string;
}

// API Functions

/**
 * Save Google OAuth2 credentials
 */
export async function saveGoogleCredentials(
  credentialsJson: string
): Promise<string> {
  return await invoke("save_google_credentials", {
    credentialsJson,
  });
}

/**
 * Check if Google credentials are configured
 */
export async function checkGoogleCredentials(): Promise<boolean> {
  return await invoke("check_google_credentials");
}

/**
 * Authenticate with Google Sheets
 */
export async function googleSheetsAuthenticate(): Promise<AuthState> {
  return await invoke("google_sheets_authenticate");
}

/**
 * Disconnect from Google Sheets
 */
export async function googleSheetsDisconnect(): Promise<string> {
  return await invoke("google_sheets_disconnect");
}

/**
 * Get current Google Sheets authentication state
 */
export async function getGoogleSheetsAuthState(): Promise<AuthState> {
  return await invoke("get_google_sheets_auth_state");
}

/**
 * Create a new spreadsheet or get existing one
 */
export async function createOrGetSpreadsheet(name: string): Promise<string> {
  return await invoke("create_or_get_spreadsheet", { name });
}

/**
 * Export a single ticker's analysis to Google Sheets
 */
export async function exportTickerToSheets(
  ticker: string,
  analysisData: TickerAnalysisData
): Promise<ExportResult> {
  return await invoke("export_ticker_to_sheets", {
    ticker,
    analysisData,
  });
}

/**
 * Export all positions to Google Sheets
 */
export async function exportAllPositionsToSheets(): Promise<ExportResult> {
  return await invoke("export_all_positions_to_sheets");
}

/**
 * Update dashboard with current portfolio data
 */
export async function updateDashboard(): Promise<string> {
  return await invoke("update_dashboard");
}

/**
 * Get the current spreadsheet URL
 */
export async function getSpreadsheetUrl(): Promise<string> {
  return await invoke("get_spreadsheet_url");
}
