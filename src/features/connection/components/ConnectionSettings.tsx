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
    <Card className="border-slate-700 bg-slate-800/50 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-white">
          <Settings className="h-5 w-5 text-blue-400" />
          Connection Settings
        </CardTitle>
        <CardDescription className="text-slate-400">
          Configure your IBKR API connection settings
        </CardDescription>
      </CardHeader>
      <CardContent className="grid grid-cols-1 gap-4 md:grid-cols-3">
        <div className="space-y-2">
          <Label htmlFor="host" className="text-slate-300">
            Host
          </Label>
          <Input
            id="host"
            value={connectionSettings.host}
            onChange={(e) => setConnectionSettings((prev) => ({ ...prev, host: e.target.value }))}
            placeholder="127.0.0.1"
            className="border-slate-600 bg-slate-900/50 text-white placeholder:text-slate-500"
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="port" className="text-slate-300">
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
            className="border-slate-600 bg-slate-900/50 text-white placeholder:text-slate-500"
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="clientId" className="text-slate-300">
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
            className="border-slate-600 bg-slate-900/50 text-white placeholder:text-slate-500"
          />
        </div>
      </CardContent>
    </Card>
  )
}
