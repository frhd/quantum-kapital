import { useState } from "react";
import { Button } from "../../../shared/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "../../../shared/components/ui/dialog";
import { Alert, AlertDescription } from "../../../shared/components/ui/alert";
import { Textarea } from "../../../shared/components/ui/textarea";
import { Label } from "../../../shared/components/ui/label";
import {
  checkGoogleCredentials,
  saveGoogleCredentials,
  googleSheetsAuthenticate,
  createOrGetSpreadsheet,
  exportTickerToSheets,
  type TickerAnalysisData,
  type ExportResult,
} from "../../../shared/api/googleSheets";
import { FileSpreadsheet, Upload, Settings, ExternalLink } from "lucide-react";

interface GoogleSheetsExportProps {
  ticker: string;
  analysisData: TickerAnalysisData;
  onExportComplete?: (result: ExportResult) => void;
}

export function GoogleSheetsExport({ ticker, analysisData, onExportComplete }: GoogleSheetsExportProps) {
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [showSetup, setShowSetup] = useState(false);
  const [credentials, setCredentials] = useState("");
  const [hasCredentials, setHasCredentials] = useState(false);
  const [exportResult, setExportResult] = useState<ExportResult | null>(null);

  const checkCredentials = async () => {
    try {
      const hasCredsential = await checkGoogleCredentials();
      setHasCredentials(hasCredsential);
      if (!hasCredsential) {
        setShowSetup(true);
      }
    } catch (err) {
      console.error("Failed to check credentials:", err);
      setError(err instanceof Error ? err.message : "Failed to check credentials");
    }
  };

  const handleSetupCredentials = async () => {
    setLoading(true);
    setError(null);
    try {
      await saveGoogleCredentials(credentials);
      setHasCredentials(true);
      setShowSetup(false);
      setSuccess("Credentials saved successfully!");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save credentials");
    } finally {
      setLoading(false);
    }
  };

  const handleExport = async () => {
    setLoading(true);
    setError(null);
    setSuccess(null);

    try {
      // Check credentials
      const hasCredsential = await checkGoogleCredentials();
      if (!hasCredsential) {
        setShowSetup(true);
        setLoading(false);
        return;
      }

      // Authenticate with Google
      await googleSheetsAuthenticate();

      // Create or get spreadsheet
      await createOrGetSpreadsheet("Quantum Kapital Analysis");

      // Export ticker data
      const result = await exportTickerToSheets(ticker, analysisData);

      setExportResult(result);
      setSuccess(result.message);

      if (onExportComplete) {
        onExportComplete(result);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Export failed");
    } finally {
      setLoading(false);
    }
  };

  const handleOpenDialog = async () => {
    setOpen(true);
    setError(null);
    setSuccess(null);
    await checkCredentials();
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm" onClick={handleOpenDialog}>
          <FileSpreadsheet className="mr-2 h-4 w-4" />
          Export to Google Sheets
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-[600px]">
        <DialogHeader>
          <DialogTitle>Export to Google Sheets</DialogTitle>
          <DialogDescription>
            Export {ticker} analysis data to a Google Sheets spreadsheet
          </DialogDescription>
        </DialogHeader>

        {showSetup && !hasCredentials ? (
          <div className="space-y-4">
            <Alert>
              <Settings className="h-4 w-4" />
              <AlertDescription>
                <p className="mb-2">To export to Google Sheets, you need to configure OAuth2 credentials:</p>
                <ol className="list-decimal list-inside space-y-1 text-sm">
                  <li>Go to Google Cloud Console</li>
                  <li>Create a new project or select existing one</li>
                  <li>Enable Google Sheets API</li>
                  <li>Create OAuth2 credentials (Desktop app)</li>
                  <li>Download the JSON and paste it below</li>
                </ol>
              </AlertDescription>
            </Alert>

            <div className="space-y-2">
              <Label htmlFor="credentials">OAuth2 Credentials JSON</Label>
              <Textarea
                id="credentials"
                placeholder='{"installed": {...}}'
                value={credentials}
                onChange={(e) => setCredentials(e.target.value)}
                rows={10}
                className="font-mono text-xs"
              />
            </div>

            <Button onClick={handleSetupCredentials} disabled={loading || !credentials}>
              <Upload className="mr-2 h-4 w-4" />
              Save Credentials
            </Button>
          </div>
        ) : (
          <div className="space-y-4">
            {error && (
              <Alert variant="destructive">
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            )}

            {success && exportResult && (
              <Alert>
                <AlertDescription>
                  <p className="mb-2">{success}</p>
                  <Button
                    variant="link"
                    size="sm"
                    className="p-0 h-auto"
                    onClick={() => window.open(exportResult.spreadsheet_url, "_blank")}
                  >
                    <ExternalLink className="mr-1 h-3 w-3" />
                    Open Spreadsheet
                  </Button>
                </AlertDescription>
              </Alert>
            )}

            {!success && (
              <p className="text-sm text-muted-foreground">
                This will create a new sheet for <strong>{ticker}</strong> in your Quantum Kapital Analysis
                spreadsheet with:
              </p>
            )}

            <ul className="text-sm space-y-1 list-disc list-inside text-muted-foreground">
              <li>Company overview and key metrics</li>
              <li>Historical financials</li>
              <li>Forward projections (Base, Bear, Bull)</li>
            </ul>

            {!hasCredentials && (
              <Button variant="outline" size="sm" onClick={() => setShowSetup(true)}>
                <Settings className="mr-2 h-4 w-4" />
                Configure Credentials
              </Button>
            )}
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)} disabled={loading}>
            Cancel
          </Button>
          {hasCredentials && !showSetup && (
            <Button onClick={handleExport} disabled={loading}>
              {loading ? "Exporting..." : "Export"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
