import { lazy, Suspense, useEffect, useState } from "react"
import {
  IconActivity,
  IconCards,
  IconChartBar,
  IconChevronDown,
  IconDeviceDesktop,
  IconHelpCircle,
  IconMoon,
  IconPlus,
  IconServer,
  IconSettings,
  IconSun,
} from "@tabler/icons-react"

import { ConnectionDialog } from "@/components/connection-dialog"
import { ThemeProvider, useTheme } from "@/components/theme-provider"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import { TooltipProvider } from "@/components/ui/tooltip"
import { demoSession } from "@/data/demo"
import type {
  AppScreen,
  ConnectionProfileViewModel,
} from "@/lib/solve-contract"
import { localProfile } from "@/lib/solve-contract"
import { cn } from "@/lib/utils"

const SetupScreen = lazy(() =>
  import("@/screens/setup-screen").then((module) => ({
    default: module.SetupScreen,
  }))
)
const SolveScreen = lazy(() =>
  import("@/screens/solve-screen").then((module) => ({
    default: module.SolveScreen,
  }))
)
const ResultsScreen = lazy(() =>
  import("@/screens/results-screen").then((module) => ({
    default: module.ResultsScreen,
  }))
)

const navigation = [
  {
    id: "setup" as const,
    hash: "#/setup",
    label: "設定",
    detail: "Solveを作成",
    icon: IconPlus,
  },
  {
    id: "solving" as const,
    hash: "#/solve/demo-run",
    label: "Solve中",
    detail: "進捗と戦略",
    icon: IconActivity,
  },
  {
    id: "results" as const,
    hash: "#/results/demo-result",
    label: "結果",
    detail: "戦略を分析",
    icon: IconChartBar,
  },
]

function screenFromHash(hash: string): AppScreen {
  if (hash.startsWith("#/solve")) {
    return "solving"
  }
  if (hash.startsWith("#/results")) {
    return "results"
  }
  return "setup"
}

function SolversApp() {
  const [screen, setScreen] = useState<AppScreen>(() =>
    screenFromHash(window.location.hash)
  )
  const [profile, setProfile] =
    useState<ConnectionProfileViewModel>(localProfile)
  const [runProfile, setRunProfile] =
    useState<ConnectionProfileViewModel>(localProfile)
  const [connectionOpen, setConnectionOpen] = useState(false)
  const { theme, setTheme } = useTheme()
  const isDark = theme === "dark"
  const displayedProfile = screen === "setup" ? profile : runProfile

  useEffect(() => {
    const onHashChange = () => setScreen(screenFromHash(window.location.hash))
    window.addEventListener("hashchange", onHashChange)
    if (!window.location.hash) {
      window.location.hash = "#/setup"
    }
    return () => window.removeEventListener("hashchange", onHashChange)
  }, [])

  const navigate = (next: AppScreen) => {
    const item = navigation.find((entry) => entry.id === next)
    if (item) {
      window.location.assign(item.hash)
    }
  }

  const navigateFromShell = (next: AppScreen) => {
    if (screen === "setup" && next !== "setup") {
      setRunProfile(profile)
    }
    navigate(next)
  }

  const startDemo = () => {
    setRunProfile(profile)
    navigate("solving")
  }

  return (
    <TooltipProvider>
      <a className="skip-link" href="#main-content">
        メインコンテンツへ移動
      </a>
      <div className="app-shell">
        <header className="topbar">
          <div className="brand">
            <span className="brand-mark">
              <IconCards />
            </span>
            <span>
              <strong>Solvers</strong>
              <small>Strategy Studio</small>
            </span>
          </div>

          <div className="topbar-context">
            <span className="hidden text-xs text-muted-foreground sm:inline">
              {screen === "setup"
                ? "New solve"
                : `${demoSession.name} · ${demoSession.id}`}
            </span>
            <Badge variant="outline" className="hidden sm:inline-flex">
              FIXTURE
            </Badge>
            {screen !== "setup" ? (
              <Badge
                variant={screen === "solving" ? "default" : "secondary"}
                className="hidden sm:inline-flex"
              >
                {screen === "solving" ? "DEMO RUNNING" : "SWEEP LIMIT"}
              </Badge>
            ) : null}
          </div>

          <div className="topbar-actions">
            <Button
              variant="outline"
              size="sm"
              className="connection-button"
              onClick={() => setConnectionOpen(true)}
              disabled={screen !== "setup"}
              title={
                screen === "setup"
                  ? "Solve先を選択"
                  : "実行デモのSolve先は開始時のプロファイルに固定されています"
              }
            >
              <span
                className={cn(
                  "connection-indicator",
                  "connection-indicator--preview"
                )}
                aria-hidden="true"
              />
              {displayedProfile.kind === "remote" ? (
                <IconServer />
              ) : (
                <IconDeviceDesktop />
              )}
              <span className="max-w-28 truncate">{displayedProfile.name}</span>
              <IconChevronDown />
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={
                isDark ? "ライトテーマに切り替え" : "ダークテーマに切り替え"
              }
              onClick={() => setTheme(isDark ? "light" : "dark")}
            >
              {isDark ? <IconSun /> : <IconMoon />}
            </Button>
          </div>
        </header>

        <aside className="sidebar" aria-label="メインナビゲーション">
          <nav>
            <p className="sidebar-label">WORKFLOW</p>
            {navigation.map((item, index) => {
              const Icon = item.icon
              const isActive = screen === item.id
              return (
                <button
                  type="button"
                  className={cn("nav-item", isActive && "nav-item--active")}
                  key={item.id}
                  aria-current={isActive ? "page" : undefined}
                  onClick={() => navigateFromShell(item.id)}
                >
                  <span className="nav-number">0{index + 1}</span>
                  <span className="nav-icon">
                    <Icon />
                  </span>
                  <span className="nav-copy">
                    <strong>{item.label}</strong>
                    <small>{item.detail}</small>
                  </span>
                  {item.id === "solving" && screen === "solving" ? (
                    <span
                      className="status-dot status-dot--live"
                      aria-hidden="true"
                    />
                  ) : null}
                </button>
              )
            })}
          </nav>

          <div className="sidebar-footer">
            <Separator />
            <button
              type="button"
              className="utility-link opacity-50"
              disabled
              title="GUI fixtureでは利用できません"
            >
              <IconSettings />
              環境設定
            </button>
            <button
              type="button"
              className="utility-link opacity-50"
              disabled
              title="GUI fixtureでは利用できません"
            >
              <IconHelpCircle />
              ガイド
            </button>
            <div className="version-block">
              <span>solvers 0.1.0</span>
              <small>GUI prototype · bdvw9nmi</small>
            </div>
          </div>
        </aside>

        <main id="main-content" className="main-content" tabIndex={-1}>
          <Suspense
            fallback={
              <div className="screen-stack text-xs text-muted-foreground">
                画面を読み込み中…
              </div>
            }
          >
            {screen === "setup" ? (
              <SetupScreen
                profile={profile}
                onStart={startDemo}
                onOpenConnections={() => setConnectionOpen(true)}
              />
            ) : null}
            {screen === "solving" ? (
              <SolveScreen
                profile={runProfile}
                onViewResults={() => navigate("results")}
              />
            ) : null}
            {screen === "results" ? (
              <ResultsScreen profile={runProfile} runId={demoSession.id} />
            ) : null}
          </Suspense>
        </main>
      </div>

      <ConnectionDialog
        open={connectionOpen}
        onOpenChange={setConnectionOpen}
        value={profile}
        onChange={setProfile}
      />
    </TooltipProvider>
  )
}

export default function App() {
  return (
    <ThemeProvider defaultTheme="light" storageKey="solvers-ui-theme">
      <SolversApp />
    </ThemeProvider>
  )
}
