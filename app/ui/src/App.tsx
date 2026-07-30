import { lazy, Suspense, useEffect, useMemo, useState } from "react"
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
import type { ConnectionProfileViewModel } from "@/lib/solve-contract"
import { localProfile } from "@/lib/solve-contract"
import {
  createNativeGateway,
  type NativeJobSnapshot,
} from "@/lib/native-gateway"
import {
  hashForRoute,
  navigateTo,
  routeFromHash,
  type AppRoute,
} from "@/lib/routes"
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

type JobBinding = {
  jobId: string
  name: string
  profile: ConnectionProfileViewModel
}

const navigation = [
  {
    id: "setup" as const,
    label: "設定",
    detail: "Solveを作成",
    icon: IconPlus,
  },
  {
    id: "solving" as const,
    label: "Solve中",
    detail: "進捗と戦略",
    icon: IconActivity,
  },
  {
    id: "results" as const,
    label: "結果",
    detail: "戦略を分析",
    icon: IconChartBar,
  },
]

function routeForNavigation(
  screen: AppRoute["screen"],
  jobId?: string
): AppRoute | null {
  if (screen === "setup") {
    return { screen }
  }
  return jobId ? { screen, jobId } : null
}

function SolversApp() {
  const gateway = useMemo(() => createNativeGateway(), [])
  const [route, setRoute] = useState<AppRoute>(() =>
    routeFromHash(window.location.hash)
  )
  const [profile, setProfile] =
    useState<ConnectionProfileViewModel>(localProfile)
  const [binding, setBinding] = useState<JobBinding | null>(() => {
    const initial = routeFromHash(window.location.hash)
    if (initial.screen === "setup") {
      return null
    }
    return {
      jobId: initial.jobId,
      name: initial.jobId,
      profile: localProfile,
    }
  })
  const [connectionOpen, setConnectionOpen] = useState(false)
  const { theme, setTheme } = useTheme()
  const isDark = theme === "dark"
  const currentJobId = route.screen === "setup" ? binding?.jobId : route.jobId
  const displayedProfile =
    route.screen === "setup" ? profile : (binding?.profile ?? localProfile)

  useEffect(() => {
    const onHashChange = () => setRoute(routeFromHash(window.location.hash))
    window.addEventListener("hashchange", onHashChange)
    if (!window.location.hash) {
      window.location.hash = hashForRoute({ screen: "setup" })
    }
    return () => window.removeEventListener("hashchange", onHashChange)
  }, [])

  const bindJob = (
    job: NativeJobSnapshot,
    name: string,
    destination: "solving" | "results",
    boundProfile: ConnectionProfileViewModel
  ) => {
    setBinding({ jobId: job.id, name, profile: boundProfile })
    navigateTo({ screen: destination, jobId: job.id })
  }

  const navigateFromShell = (screen: AppRoute["screen"]) => {
    const next = routeForNavigation(screen, currentJobId)
    if (next) {
      navigateTo(next)
    }
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
              {route.screen === "setup"
                ? "New solve"
                : `${binding?.name ?? route.jobId} · ${route.jobId}`}
            </span>
            <Badge
              variant={gateway.availability.available ? "outline" : "secondary"}
              className="hidden sm:inline-flex"
            >
              {gateway.availability.available ? "DESKTOP" : "BROWSER PREVIEW"}
            </Badge>
            {route.screen !== "setup" ? (
              <Badge
                variant={route.screen === "solving" ? "default" : "secondary"}
                className="hidden sm:inline-flex"
              >
                {route.screen === "solving" ? "JOB" : "RESULT"}
              </Badge>
            ) : null}
          </div>

          <div className="topbar-actions">
            <Button
              variant="outline"
              size="sm"
              className="connection-button"
              onClick={() => setConnectionOpen(true)}
              disabled={route.screen !== "setup"}
              title={
                route.screen === "setup"
                  ? "Solve先を選択"
                  : "job作成後のSolve先は固定されています"
              }
            >
              <span
                className={cn(
                  "connection-indicator",
                  displayedProfile.kind === "local" &&
                    gateway.availability.available &&
                    "connection-indicator--online"
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
              const isActive = route.screen === item.id
              const destination = routeForNavigation(item.id, currentJobId)
              return (
                <button
                  type="button"
                  className={cn("nav-item", isActive && "nav-item--active")}
                  key={item.id}
                  aria-current={isActive ? "page" : undefined}
                  onClick={() => navigateFromShell(item.id)}
                  disabled={!destination}
                  title={
                    destination
                      ? undefined
                      : "Solveを開始するか、生成済みsolutionを開いてください"
                  }
                >
                  <span className="nav-number">0{index + 1}</span>
                  <span className="nav-icon">
                    <Icon />
                  </span>
                  <span className="nav-copy">
                    <strong>{item.label}</strong>
                    <small>{item.detail}</small>
                  </span>
                  {item.id === "solving" && isActive ? (
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
              title="今後の設定画面で提供します"
            >
              <IconSettings />
              環境設定
            </button>
            <button
              type="button"
              className="utility-link opacity-50"
              disabled
              title="ガイドは未収録です"
            >
              <IconHelpCircle />
              ガイド
            </button>
            <div className="version-block">
              <span>solvers 0.1.0</span>
              <small>Desktop GUI · bdvw9nmi</small>
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
            {route.screen === "setup" ? (
              <SetupScreen
                profile={profile}
                gateway={gateway}
                onJobStarted={(job, name) =>
                  bindJob(job, name, "solving", profile)
                }
                onCheckpointResumed={(job, name) =>
                  bindJob(job, name, "solving", localProfile)
                }
                onResultOpened={(job, name) =>
                  bindJob(job, name, "results", localProfile)
                }
                onOpenConnections={() => setConnectionOpen(true)}
              />
            ) : null}
            {route.screen === "solving" ? (
              <SolveScreen
                profile={displayedProfile}
                gateway={gateway}
                jobId={route.jobId}
                displayName={
                  binding?.jobId === route.jobId ? binding.name : route.jobId
                }
                onViewResults={() =>
                  navigateTo({ screen: "results", jobId: route.jobId })
                }
              />
            ) : null}
            {route.screen === "results" ? (
              <ResultsScreen
                profile={displayedProfile}
                gateway={gateway}
                jobId={route.jobId}
                displayName={
                  binding?.jobId === route.jobId ? binding.name : route.jobId
                }
                onJobResumed={(job, name) =>
                  bindJob(job, name, "solving", localProfile)
                }
              />
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
