# Settings System Guide

## Overview

Quantum Kapital now has a **persistent settings system** that saves your configuration to disk and loads it automatically on startup.

## Settings File Location

Settings are stored as JSON in:
- **macOS**: `~/Library/Application Support/quantum-kapital/settings.json`
- **Linux**: `~/.config/quantum-kapital/settings.json`
- **Windows**: `%APPDATA%\quantum-kapital\settings.json`

## Settings Structure

The settings JSON has the following structure:

```json
{
  "ibkr": {
    "default_host": "127.0.0.1",
    "default_port": 4004,
    "default_client_id": 100,
    "connection_timeout_ms": 30000,
    "reconnect_interval_ms": 5000,
    "max_reconnect_attempts": 3,
    "rate_limit_per_second": 50
  },
  "logging": {
    "level": "info",
    "file_path": null,
    "max_file_size_mb": 10,
    "max_files": 5,
    "console_output": true
  },
  "ui": {
    "theme": "dark",
    "default_refresh_interval_ms": 1000,
    "show_notifications": true,
    "auto_save_layout": true
  },
  "api": {
    "alpha_vantage_api_key": null
  },
  "google_sheets": {
    "spreadsheet_id": null,
    "spreadsheet_name": "Quantum Kapital Analysis",
    "auto_export": false,
    "last_export_timestamp": null
  }
}
```

## Using Settings in Frontend

### 1. Import the Settings API

```typescript
import { getSettings, updateSettings, getSettingsPath } from '@/shared/api/settings';
import type { AppConfig } from '@/shared/api/settings';
```

### 2. Read Settings

```typescript
// Get all settings
const settings = await getSettings();
console.log('Current theme:', settings.ui.theme);
console.log('IBKR host:', settings.ibkr.default_host);

// Get settings file path (for debugging)
const path = await getSettingsPath();
console.log('Settings file:', path);
```

### 3. Update Settings

```typescript
// Get current settings
const settings = await getSettings();

// Modify what you need
settings.ui.theme = 'light';
settings.ibkr.default_port = 7497;
settings.google_sheets.auto_export = true;

// Save back to disk
await updateSettings(settings);
```

### 4. Update Google Sheets Spreadsheet

```typescript
import { updateGoogleSheetsSpreadsheet, getGoogleSheetsSpreadsheet } from '@/shared/api/settings';

// After creating a spreadsheet
await updateGoogleSheetsSpreadsheet(
  'your-spreadsheet-id-here',
  'My Analysis Spreadsheet'
);

// Later, retrieve it
const spreadsheetId = await getGoogleSheetsSpreadsheet();
if (spreadsheetId) {
  console.log('Using spreadsheet:', spreadsheetId);
}
```

## Example: Creating a Settings Page

```typescript
import { useState, useEffect } from 'react';
import { getSettings, updateSettings } from '@/shared/api/settings';
import type { AppConfig } from '@/shared/api/settings';

function SettingsPage() {
  const [settings, setSettings] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);

  // Load settings on mount
  useEffect(() => {
    async function loadSettings() {
      try {
        const config = await getSettings();
        setSettings(config);
      } catch (error) {
        console.error('Failed to load settings:', error);
      } finally {
        setLoading(false);
      }
    }
    loadSettings();
  }, []);

  // Save settings
  const handleSave = async () => {
    if (!settings) return;

    try {
      await updateSettings(settings);
      alert('Settings saved!');
    } catch (error) {
      console.error('Failed to save settings:', error);
      alert('Failed to save settings');
    }
  };

  if (loading) return <div>Loading settings...</div>;
  if (!settings) return <div>Failed to load settings</div>;

  return (
    <div>
      <h1>Settings</h1>

      {/* Theme Selection */}
      <div>
        <label>Theme:</label>
        <select
          value={settings.ui.theme}
          onChange={(e) =>
            setSettings({
              ...settings,
              ui: { ...settings.ui, theme: e.target.value },
            })
          }
        >
          <option value="dark">Dark</option>
          <option value="light">Light</option>
        </select>
      </div>

      {/* IBKR Port */}
      <div>
        <label>IBKR Port:</label>
        <input
          type="number"
          value={settings.ibkr.default_port}
          onChange={(e) =>
            setSettings({
              ...settings,
              ibkr: { ...settings.ibkr, default_port: parseInt(e.target.value) },
            })
          }
        />
      </div>

      {/* Auto Export */}
      <div>
        <label>
          <input
            type="checkbox"
            checked={settings.google_sheets.auto_export}
            onChange={(e) =>
              setSettings({
                ...settings,
                google_sheets: {
                  ...settings.google_sheets,
                  auto_export: e.target.checked,
                },
              })
            }
          />
          Auto-export to Google Sheets
        </label>
      </div>

      <button onClick={handleSave}>Save Settings</button>
    </div>
  );
}
```

## Backend Usage (Rust)

### Reading Settings

```rust
use crate::config::SettingsState;
use tauri::State;

#[tauri::command]
async fn my_command(settings_state: State<'_, SettingsState>) -> Result<String, String> {
    let config = settings_state.config.read().await;

    // Access settings
    let theme = &config.ui.theme;
    let port = config.ibkr.default_port;

    Ok(format!("Theme: {}, Port: {}", theme, port))
}
```

### Updating Settings

```rust
#[tauri::command]
async fn update_theme(
    theme: String,
    settings_state: State<'_, SettingsState>
) -> Result<(), String> {
    let mut config = settings_state.config.write().await;
    config.ui.theme = theme;

    // Save to disk
    config.save()
        .await
        .map_err(|e| format!("Failed to save: {}", e))?;

    Ok(())
}
```

## Available Settings

### IBKR Settings (`ibkr`)
- `default_host`: TWS/Gateway hostname
- `default_port`: TWS/Gateway port (4004 for live, 7497 for paper)
- `default_client_id`: Client ID for IBKR connection
- `connection_timeout_ms`: Connection timeout in milliseconds
- `reconnect_interval_ms`: Auto-reconnect interval
- `max_reconnect_attempts`: Max reconnection attempts
- `rate_limit_per_second`: API rate limit

### Logging Settings (`logging`)
- `level`: Log level ("debug", "info", "warn", "error")
- `file_path`: Optional log file path
- `max_file_size_mb`: Max log file size before rotation
- `max_files`: Max number of rotated log files
- `console_output`: Enable console logging

### UI Settings (`ui`)
- `theme`: UI theme ("dark" or "light")
- `default_refresh_interval_ms`: Data refresh interval
- `show_notifications`: Enable notifications
- `auto_save_layout`: Auto-save window layout

### API Settings (`api`)
- `alpha_vantage_api_key`: Alpha Vantage API key for fundamental data

### Google Sheets Settings (`google_sheets`)
- `spreadsheet_id`: Current spreadsheet ID
- `spreadsheet_name`: Spreadsheet name
- `auto_export`: Automatically export after analysis
- `last_export_timestamp`: Last export timestamp

## Default Values

If the settings file doesn't exist, these defaults are used:
- Theme: `dark`
- IBKR Port: `4004` (live trading)
- IBKR Client ID: `100`
- Auto-export: `false`
- Notifications: `true`

## Manual Editing

You can manually edit the `settings.json` file with any text editor. The app will load the changes on next startup.

**Pro tip**: To reset to defaults, simply delete the `settings.json` file.

## Troubleshooting

### Settings not persisting?
- Check file permissions on the config directory
- Look for error messages in the console
- Verify the settings file path with `getSettingsPath()`

### Settings file corrupted?
- Delete `settings.json` and restart the app
- Defaults will be created automatically

### Want to see current settings?
```typescript
const path = await getSettingsPath();
console.log('Settings location:', path);
const settings = await getSettings();
console.log('Current settings:', JSON.stringify(settings, null, 2));
```
