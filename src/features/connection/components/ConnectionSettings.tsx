import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../shared/components/ui/card"
import { Input } from "../../../shared/components/ui/input"
import { Label } from "../../../shared/components/ui/label"
import { Settings } from "lucide-react"
import type { ConnectionConfig } from "../../../shared/types"

interface ConnectionSettingsProps {
  connectionSettings: ConnectionConfig
  setConnectionSettings: React.Dispatch<React.SetStateAction<ConnectionConfig>>
}

export function ConnectionSettings({
  connectionSettings,
  setConnectionSettings,
}: ConnectionSettingsProps) {
  return (
    <Card className="border-border bg-card/50 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="text-foreground flex items-center gap-2">
          <Settings className="h-5 w-5 text-blue-400" />
          Connection Settings
        </CardTitle>
        <CardDescription className="text-muted-foreground">
          Configure your IBKR API connection settings
        </CardDescription>
      </CardHeader>
      <CardContent className="grid grid-cols-1 gap-4 md:grid-cols-3">
        <div className="space-y-2">
          <Label htmlFor="host" className="text-foreground">
            Host
          </Label>
          <Input
            id="host"
            value={connectionSettings.host}
            onChange={(e) => setConnectionSettings((prev) => ({ ...prev, host: e.target.value }))}
            placeholder="127.0.0.1"
            className="border-input bg-background/50 text-foreground placeholder:text-muted-foreground"
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="port" className="text-foreground">
            Port
          </Label>
          <Input
            id="port"
            type="number"
            value={connectionSettings.port}
            onChange={(e) =>
              setConnectionSettings((prev) => ({ ...prev, port: parseInt(e.target.value) || 4004 }))
            }
            placeholder="4004"
            className="border-input bg-background/50 text-foreground placeholder:text-muted-foreground"
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="clientId" className="text-foreground">
            Client ID
          </Label>
          <Input
            id="clientId"
            type="number"
            value={connectionSettings.client_id}
            onChange={(e) =>
              setConnectionSettings((prev) => ({
                ...prev,
                client_id: parseInt(e.target.value) || 100,
              }))
            }
            placeholder="100"
            className="border-input bg-background/50 text-foreground placeholder:text-muted-foreground"
          />
        </div>
      </CardContent>
    </Card>
  )
}
