import { useCallback, useEffect, useState } from "react";
import { ApiError, api, storeToken, storedToken } from "./api/client";
import { PROTOCOL_VERSION, type ServerInfo } from "./api/types";
import { Notice } from "./components/common";
import { Badge, Button, Dialog, Icon, Input, Sidebar, SidebarItem } from "./cryst";
import { SetupPage } from "./pages/Setup";
import { RunsPage } from "./pages/Runs";
import { ResultsPage } from "./pages/Results";
import { GuidePage } from "./pages/Guide";
import { parseRoute } from "./route";

type Connection = { status: "ok"; info: ServerInfo } | { status: "unauthorized" | "offline"; message: string };

const NAV = [
  { page: "setup", label: "Setup", icon: "adjustments-horizontal" },
  { page: "runs", label: "Runs", icon: "list-details" },
  { page: "results", label: "Results", icon: "chart-grid-dots" },
  { page: "guide", label: "Guide", icon: "book" },
] as const;

export function App() {
  const [route, setRoute] = useState(() => parseRoute(window.location.hash));
  const [connection, setConnection] = useState<Connection | null>(null);
  const [tokenDialog, setTokenDialog] = useState(false);

  useEffect(() => {
    const onHash = () => setRoute(parseRoute(window.location.hash));
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const probe = useCallback(async () => {
    try {
      setConnection({ status: "ok", info: await api.info() });
    } catch (error) {
      const unauthorized = error instanceof ApiError && error.code === "unauthorized";
      setConnection({ status: unauthorized ? "unauthorized" : "offline", message: (error as Error).message });
      if (unauthorized) setTokenDialog(true);
    }
  }, []);

  useEffect(() => {
    void probe();
    const timer = setInterval(probe, 10_000);
    return () => clearInterval(timer);
  }, [probe]);

  const info = connection?.status === "ok" ? connection.info : null;
  return (
    <div className="app">
      <Sidebar
        label="Main"
        className="app-sidebar"
        header={<span className="brand">Solvers</span>}
        footer={
          <>
            {info && info.protocolVersion !== PROTOCOL_VERSION && (
              <Notice
                variant="warning"
                className="protocol-alert"
                title="Protocol mismatch"
                description={`daemon v${info.protocolVersion}, GUI v${PROTOCOL_VERSION}`}
              />
            )}
            <button
              type="button"
              className="connection"
              onClick={() => setTokenDialog(true)}
              title="Connection settings"
            >
              <Icon name="server" />
              <span className="mono grow host">{window.location.host}</span>
              {connection === null ? null : connection.status === "ok" ? (
                <Badge variant="success" dot>
                  Connected
                </Badge>
              ) : (
                <Badge variant="destructive" dot title={connection.message}>
                  {connection.status === "unauthorized" ? "Unauthorized" : "Disconnected"}
                </Badge>
              )}
            </button>
          </>
        }
      >
        {NAV.map((item) => (
          <SidebarItem key={item.page} icon={item.icon} href={`#/${item.page}`} active={route.page === item.page}>
            {item.label}
          </SidebarItem>
        ))}
      </Sidebar>
      <div className="page">
        {route.page === "setup" && <SetupPage />}
        {route.page === "runs" && <RunsPage runId={route.runId} maxConcurrent={info?.maxConcurrentRuns} />}
        {route.page === "results" && <ResultsPage runId={route.runId} />}
        {route.page === "guide" && <GuidePage />}
      </div>
      {tokenDialog && (
        <TokenDialog
          onClose={() => setTokenDialog(false)}
          onSave={(token) => {
            storeToken(token);
            setTokenDialog(false);
            void probe();
          }}
        />
      )}
    </div>
  );
}

// Mounted only while open, so each opening starts from the stored token.
function TokenDialog({ onClose, onSave }: { onClose: () => void; onSave: (token: string) => void }) {
  const [token, setToken] = useState(storedToken);
  return (
    <Dialog
      open
      onOpenChange={(next) => !next && onClose()}
      icon="key"
      title="Connect to daemon"
      description="Enter the token solversd displays at startup. Opening the URL on the gui: line sets it automatically."
      closeLabel="Close"
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="token-form">
            Save
          </Button>
        </>
      }
    >
      <form
        id="token-form"
        className="mono"
        onSubmit={(event) => {
          event.preventDefault();
          onSave(token.trim());
        }}
      >
        <Input label="Token" value={token} onChange={(e) => setToken(e.target.value)} spellCheck={false} />
      </form>
    </Dialog>
  );
}
