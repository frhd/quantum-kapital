# Google Sheets Dashboard Guide

## 📊 Dashboard Overview

When you export analysis data to Google Sheets, Quantum Kapital creates a **Dashboard** sheet that serves as your main navigation hub.

## 🎯 How to Navigate to Ticker Sheets

### **Option 1: Click the Links (Recommended)**

The dashboard has a **"Click to View"** column with clickable links for each ticker:

```
┌──────────┬──────────────────┬──────────────────┐
│ Ticker   │ Click to View    │ Last Updated     │
├──────────┼──────────────────┼──────────────────┤
│ AAPL     │ → View AAPL      │ 2025-11-02 ...   │ ← Click this!
│ GOOGL    │ → View GOOGL     │ 2025-11-02 ...   │
│ MSFT     │ → View MSFT      │ 2025-11-02 ...   │
└──────────┴──────────────────┴──────────────────┘
```

**Just click** on any "→ View TICKER" link to jump directly to that ticker's analysis sheet!

### **Option 2: Use Sheet Tabs**

At the bottom of the Google Sheet, you'll see tabs for each sheet:

```
[Dashboard] [AAPL] [GOOGL] [MSFT]
     ↑        ↑      ↑       ↑
   Click any tab to switch sheets
```

### **Option 3: Search for Sheet**

Press `Ctrl+F` (or `Cmd+F` on Mac) to search, then type the ticker name to find its sheet.

## 📋 Dashboard Layout

```
┌─────────────────────────────────────────────────────┐
│ Quantum Kapital - Portfolio Analysis Dashboard      │
├─────────────────────────────────────────────────────┤
│                                                      │
│ Click a ticker below to view its analysis →         │
│                                          Total Pos: 5│
│                                      Total Value: $X │
│                                                      │
├──────────┬──────────────────┬──────────────────────┤
│ Ticker   │ Click to View    │ Last Updated         │
├──────────┼──────────────────┼──────────────────────┤
│ AAPL     │ → View AAPL      │ 2025-11-02 15:30 UTC │
│ GOOGL    │ → View GOOGL     │ 2025-11-02 15:30 UTC │
│ MSFT     │ → View MSFT      │ 2025-11-02 15:30 UTC │
└──────────┴──────────────────┴──────────────────────┘
```

## 🔄 Updating the Dashboard

The dashboard automatically updates when you:
1. Export a new ticker analysis
2. Use the "Update Dashboard" command (if available)

Each time you export, the dashboard shows:
- **Updated ticker list** - All analyzed tickers
- **Current portfolio value** - Sum of all positions
- **Total positions** - Number of positions
- **Last updated timestamp** - When the export happened

## 📄 Ticker Sheet Structure

When you click on a ticker link, you'll see:

```
┌─────────────────────────────────────┐
│ Company Overview                     │
├─────────────────────────────────────┤
│ Ticker:       AAPL                  │
│ Company:      Apple Inc.            │
│ Sector:       Technology            │
│ Market Cap:   $2.8T                 │
│ Current Price: $175.00              │
│ P/E Ratio:    28.5                  │
│ EPS:          $6.15                 │
├─────────────────────────────────────┤
│ Historical Financials               │
├──────┬──────────┬────────┬─────────┤
│ Year │ Revenue  │ Net In │ EPS     │
├──────┼──────────┼────────┼─────────┤
│ 2023 │ $383.3B  │ $96.9B │ $6.15   │
│ 2022 │ $394.3B  │ $99.8B │ $6.11   │
│ ...                                 │
├─────────────────────────────────────┤
│ Forward Projections                 │
├──────────┬────────┬─────────┬───────┤
│ Scenario │ Target │ Upside% │ ...   │
├──────────┼────────┼─────────┼───────┤
│ Base     │ $225   │ 28.5%   │ ...   │
│ Bear     │ $180   │  2.8%   │ ...   │
│ Bull     │ $280   │ 60.0%   │ ...   │
└──────────┴────────┴─────────┴───────┘
```

## 💡 Tips

### **Tip 1: Organize Your Tickers**

The ticker list shows in the order they were exported. To reorder:
1. Manually cut and paste rows in the Dashboard sheet
2. Or delete old tickers and re-export in your preferred order

### **Tip 2: Add Notes**

The dashboard is fully editable! You can:
- Add a notes column
- Color-code tickers by sector
- Add your own formulas
- Create charts from the data

### **Tip 3: Share with Team**

1. Click the **Share** button in Google Sheets (top right)
2. Add team members' emails
3. Set permissions (Viewer, Commenter, or Editor)
4. They can view all analysis data and click the same links!

### **Tip 4: Create a Shortcut**

Bookmark the spreadsheet URL for quick access:
```
https://docs.google.com/spreadsheets/d/YOUR_SPREADSHEET_ID/edit
```

### **Tip 5: Use on Mobile**

The Google Sheets mobile app works great!
- Tap any "→ View TICKER" link to navigate
- Swipe left/right to switch between sheets
- View all your analysis on the go

## 🔧 Troubleshooting

### **Links Don't Work?**

If clicking "→ View AAPL" doesn't navigate:

1. **Check if the sheet exists**: Look at the bottom tabs
2. **Try clicking the tab directly**: Click "AAPL" at the bottom
3. **Re-export the ticker**: This will recreate the sheet

### **Dashboard Out of Date?**

The dashboard shows data from the last export. To refresh:
1. Export tickers again from Quantum Kapital
2. The dashboard will auto-update with new data

### **Can I Customize the Dashboard?**

Yes! The dashboard is fully editable:
- Add/remove columns
- Change colors and formatting
- Add your own formulas
- Create charts

**Note**: If you re-export, custom changes may be overwritten.

### **Want a Separate Dashboard?**

You can:
1. Duplicate the Dashboard sheet
2. Rename it (e.g., "My Custom Dashboard")
3. Keep it as a template
4. The original Dashboard will still update on exports

## 📱 Mobile Access

The dashboard works perfectly on mobile:

**iOS (iPhone/iPad):**
1. Open Google Sheets app
2. Find "Quantum Kapital Analysis"
3. Tap any ticker link
4. View full analysis

**Android:**
1. Open Google Sheets app
2. Navigate to spreadsheet
3. Tap links to view tickers

## 🎨 Customization Ideas

### **Add a Summary Section**
```
Total Portfolio Value:  $125,432
Best Performer:        NVDA (+45%)
Worst Performer:       AAPL (-5%)
```

### **Create a Chart**
1. Select ticker data
2. Insert > Chart
3. Choose chart type (bar, line, pie)
4. Place on Dashboard

### **Add Conditional Formatting**
1. Select "Upside %" column
2. Format > Conditional formatting
3. Green for >20%, Red for <5%

### **Create a Quick Summary View**
Add a pivot table showing:
- Total value by sector
- Average P/E by ticker
- Target prices vs current

## 🚀 Advanced: Custom Formulas

You can add formulas to the dashboard:

### **Auto-calculate total upside:**
```
=AVERAGE('AAPL'!C18, 'GOOGL'!C18, 'MSFT'!C18)
```

### **Show highest target price:**
```
=MAX('AAPL'!B18, 'GOOGL'!B18, 'MSFT'!B18)
```

### **Count analyzed stocks:**
```
=COUNTA(A7:A100) - 1
```

---

## Summary

✅ **Click "→ View TICKER"** links in the "Click to View" column
✅ **Or use sheet tabs** at the bottom
✅ **Dashboard auto-updates** on each export
✅ **Fully customizable** - add your own columns, charts, formulas
✅ **Works on mobile** via Google Sheets app
✅ **Shareable** with team members

Enjoy your organized analysis dashboard! 📊
