import {
  Activity,
  BarChart3,
  Eye,
  FileText,
  Inbox,
  LineChart,
  type LucideIcon,
  Menu,
  Moon,
  Search,
  Settings,
  Sun,
} from "lucide-react"
import { cn } from "../../lib/utils"
import { useTheme } from "../../hooks/useTheme"

export type PageId =
  | "analysis"
  | "scanner"
  | "candidates"
  | "tracker"
  | "research"
  | "eval"
  | "positions"
  | "account"

interface NavItem {
  id: PageId
  label: string
  icon: LucideIcon
}

interface NavGroup {
  label: string
  items: NavItem[]
}

export const NAV_GROUPS: NavGroup[] = [
  {
    label: "Markets",
    items: [
      { id: "analysis", label: "Analysis", icon: LineChart },
      { id: "scanner", label: "Scanner", icon: Search },
      { id: "candidates", label: "Candidates", icon: Inbox },
      { id: "tracker", label: "Tracker", icon: Eye },
      { id: "research", label: "Research", icon: FileText },
      { id: "eval", label: "Eval", icon: Activity },
    ],
  },
  {
    label: "Account",
    items: [
      { id: "positions", label: "Positions", icon: BarChart3 },
      { id: "account", label: "Account Details", icon: Settings },
    ],
  },
]

export const PAGE_LABELS: Record<PageId, string> = NAV_GROUPS.flatMap((g) => g.items).reduce(
  (acc, item) => {
    acc[item.id] = item.label
    return acc
  },
  {} as Record<PageId, string>,
)

interface SidebarProps {
  currentPage: PageId
  onNavigate: (page: PageId) => void
  expanded: boolean
  onToggleExpand: () => void
  badges?: Partial<Record<PageId, number>>
}

export function Sidebar({
  currentPage,
  onNavigate,
  expanded,
  onToggleExpand,
  badges,
}: SidebarProps) {
  const { theme, toggleTheme } = useTheme()
  const ThemeIcon = theme === "dark" ? Sun : Moon

  return (
    <aside
      aria-label="Main navigation"
      className={cn(
        "border-sidebar-border bg-sidebar text-sidebar-foreground flex h-full shrink-0 flex-col border-r transition-[width] duration-150 ease-in-out",
        expanded ? "w-52" : "w-12",
      )}
    >
      <div className="border-sidebar-border flex h-12 items-center justify-between border-b px-2">
        {expanded && (
          <span className="ml-1 truncate text-sm font-semibold tracking-tight">
            Quantum Kapital
          </span>
        )}
        <button
          type="button"
          onClick={onToggleExpand}
          aria-label={expanded ? "Collapse sidebar" : "Expand sidebar"}
          className="text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-foreground flex h-8 w-8 items-center justify-center rounded-sm"
        >
          <Menu className="h-4 w-4" />
        </button>
      </div>

      <nav className="flex-1 overflow-y-auto py-2">
        <ul className="flex flex-col gap-px" role="list">
          {NAV_GROUPS.map((group, gi) => (
            <li key={group.label}>
              {expanded && (
                <div className="text-muted-foreground px-3 pt-3 pb-1 text-[10px] font-semibold tracking-wider uppercase">
                  {group.label}
                </div>
              )}
              {!expanded && gi > 0 && <div className="border-sidebar-border mx-2 my-2 border-t" />}
              <ul className="flex flex-col" role="list">
                {group.items.map((item) => {
                  const Icon = item.icon
                  const active = currentPage === item.id
                  const badge = badges?.[item.id]
                  return (
                    <li key={item.id}>
                      <button
                        type="button"
                        onClick={() => onNavigate(item.id)}
                        aria-current={active ? "page" : undefined}
                        title={!expanded ? item.label : undefined}
                        className={cn(
                          "flex w-full items-center gap-3 px-3 py-2 text-sm transition-colors",
                          active
                            ? "bg-sidebar-accent text-sidebar-primary"
                            : "text-sidebar-foreground/80 hover:bg-sidebar-accent hover:text-sidebar-foreground",
                          !expanded && "justify-center px-0",
                        )}
                      >
                        <Icon className="h-4 w-4 shrink-0" />
                        {expanded && (
                          <>
                            <span className="flex-1 truncate text-left">{item.label}</span>
                            {badge !== undefined && badge > 0 && (
                              <span className="bg-primary/20 text-primary rounded-sm px-1.5 py-0.5 font-mono text-[10px] tabular-nums">
                                {badge}
                              </span>
                            )}
                          </>
                        )}
                      </button>
                    </li>
                  )
                })}
              </ul>
            </li>
          ))}
        </ul>
      </nav>

      <div className="border-sidebar-border border-t p-1">
        <button
          type="button"
          onClick={toggleTheme}
          aria-label={theme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
          title={!expanded ? (theme === "dark" ? "Light mode" : "Dark mode") : undefined}
          className={cn(
            "text-sidebar-foreground/80 hover:bg-sidebar-accent hover:text-sidebar-foreground flex w-full items-center gap-3 rounded-sm px-3 py-2 text-sm transition-colors",
            !expanded && "justify-center px-0",
          )}
        >
          <ThemeIcon className="h-4 w-4 shrink-0" />
          {expanded && (
            <span className="flex-1 truncate text-left">
              {theme === "dark" ? "Light Mode" : "Dark Mode"}
            </span>
          )}
        </button>
      </div>
    </aside>
  )
}
