import { cn } from "../../../shared/lib/utils"
import { useWorkspace } from "../context/WorkspaceContext"
import { WORKSPACE_TAB_LABELS, WORKSPACE_TAB_ORDER, type WorkspaceTabId } from "../types"

export function WorkspaceTabsNav() {
  const { tab, setTab } = useWorkspace()
  return (
    <div className="border-border flex gap-1 overflow-x-auto border-b">
      {WORKSPACE_TAB_ORDER.map((id) => (
        <TabButton key={id} id={id} active={tab === id} onSelect={setTab} />
      ))}
    </div>
  )
}

interface TabButtonProps {
  id: WorkspaceTabId
  active: boolean
  onSelect: (id: WorkspaceTabId) => void
}

function TabButton({ id, active, onSelect }: TabButtonProps) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={() => onSelect(id)}
      className={cn(
        "border-b-2 px-3 py-2 text-sm transition-colors",
        active
          ? "border-primary text-foreground"
          : "text-muted-foreground hover:text-foreground border-transparent",
      )}
    >
      {WORKSPACE_TAB_LABELS[id]}
    </button>
  )
}
