import { invoke } from "@tauri-apps/api/core";

// Types matching Rust structures

export interface AppConfig {
  ibkr: IbkrConfig;
  logging: LoggingConfig;
  ui: UiConfig;
  api: ApiConfig;
  google_sheets: GoogleSheetsConfig;
}

export interface IbkrConfig {
  default_host: string;
  default_port: number;
  default_client_id: number;
  connection_timeout_ms: number;
  reconnect_interval_ms: number;
  max_reconnect_attempts: number;
  rate_limit_per_second: number;
}

export interface LoggingConfig {
  level: string;
  file_path: string | null;
  max_file_size_mb: number;
  max_files: number;
  console_output: boolean;
}

export interface UiConfig {
  theme: string;
  default_refresh_interval_ms: number;
  show_notifications: boolean;
  auto_save_layout: boolean;
}

export interface ApiConfig {
  alpha_vantage_api_key: string | null;
}

export interface GoogleSheetsConfig {
  spreadsheet_id: string | null;
  spreadsheet_name: string;
  auto_export: boolean;
  last_export_timestamp: string | null;
}

// API Functions

/**
 * Get all application settings
 */
export async function getSettings(): Promise<AppConfig> {
  return await invoke("get_settings");
}

/**
 * Update all application settings
 */
export async function updateSettings(settings: AppConfig): Promise<void> {
  return await invoke("update_settings", { settings });
}

/**
 * Update Google Sheets spreadsheet configuration
 */
export async function updateGoogleSheetsSpreadsheet(
  spreadsheetId: string,
  spreadsheetName: string
): Promise<void> {
  return await invoke("update_google_sheets_spreadsheet", {
    spreadsheetId,
    spreadsheetName,
  });
}

/**
 * Get Google Sheets spreadsheet ID
 */
export async function getGoogleSheetsSpreadsheet(): Promise<string | null> {
  return await invoke("get_google_sheets_spreadsheet");
}

/**
 * Get the settings file path (for debugging)
 */
export async function getSettingsPath(): Promise<string> {
  return await invoke("get_settings_path");
}
