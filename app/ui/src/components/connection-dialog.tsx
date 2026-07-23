import { useState } from "react"
import {
  IconCheck,
  IconDeviceDesktop,
  IconInfoCircle,
  IconServer,
  IconShieldLock,
} from "@tabler/icons-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import type { ConnectionProfileViewModel } from "@/lib/solve-contract"
import {
  localProfile,
  remoteProfile as defaultRemoteProfile,
} from "@/lib/solve-contract"
import { cn } from "@/lib/utils"

type ConnectionDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  value: ConnectionProfileViewModel
  onChange: (profile: ConnectionProfileViewModel) => void
}

type RemoteConnectionProfile = Extract<
  ConnectionProfileViewModel,
  { kind: "remote" }
>

const remoteTemplate = defaultRemoteProfile as RemoteConnectionProfile

export function ConnectionDialog({
  open,
  onOpenChange,
  value,
  onChange,
}: ConnectionDialogProps) {
  const [kind, setKind] = useState(value.kind)
  const [name, setName] = useState(remoteTemplate.name)
  const [endpoint, setEndpoint] = useState(remoteTemplate.endpoint)

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      setKind(value.kind)
      if (value.kind === "remote") {
        setName(value.name)
        setEndpoint(value.endpoint ?? "")
      }
    }
    onOpenChange(nextOpen)
  }

  const applyProfile = () => {
    if (kind === "local") {
      onChange(localProfile)
    } else {
      onChange({
        ...remoteTemplate,
        name: name.trim() || remoteTemplate.name,
        endpoint: endpoint.trim() || remoteTemplate.endpoint,
      })
    }
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="gap-5 p-5 sm:max-w-xl">
        <DialogHeader>
          <DialogTitle className="text-base">Solve先を選択</DialogTitle>
          <DialogDescription>
            GUIとSolveエンジンの配置を分離します。設定内容と成果物の意味はどちらも同じです。
          </DialogDescription>
        </DialogHeader>

        <div
          className="grid gap-3 sm:grid-cols-2"
          role="group"
          aria-label="Solve先"
        >
          <button
            type="button"
            className={cn(
              "connection-choice",
              kind === "local" && "connection-choice--active"
            )}
            onClick={() => setKind("local")}
            aria-pressed={kind === "local"}
          >
            <span className="flex items-start justify-between gap-3">
              <span className="rounded-md border bg-background p-2">
                <IconDeviceDesktop className="size-4" />
              </span>
              {kind === "local" ? (
                <span className="rounded-full bg-foreground p-0.5 text-background">
                  <IconCheck className="size-3" />
                </span>
              ) : null}
            </span>
            <span>
              <strong>このマシン</strong>
              <small>同一バイナリ内のローカルアダプター</small>
            </span>
            <Badge variant="secondary">デモ</Badge>
          </button>

          <button
            type="button"
            className={cn(
              "connection-choice",
              kind === "remote" && "connection-choice--active"
            )}
            onClick={() => setKind("remote")}
            aria-pressed={kind === "remote"}
          >
            <span className="flex items-start justify-between gap-3">
              <span className="rounded-md border bg-background p-2">
                <IconServer className="size-4" />
              </span>
              {kind === "remote" ? (
                <span className="rounded-full bg-foreground p-0.5 text-background">
                  <IconCheck className="size-3" />
                </span>
              ) : null}
            </span>
            <span>
              <strong>リモートマシン</strong>
              <small>暗号化トンネル越しのsolvers bridge</small>
            </span>
            <Badge variant="outline">契約プレビュー</Badge>
          </button>
        </div>

        {kind === "remote" ? (
          <div className="space-y-4 rounded-lg border bg-muted/30 p-4">
            <div className="grid gap-1.5">
              <Label htmlFor="profile-name">プロファイル名</Label>
              <Input
                id="profile-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="endpoint">Bridge URL</Label>
              <Input
                id="endpoint"
                inputMode="url"
                value={endpoint}
                onChange={(event) => setEndpoint(event.target.value)}
              />
              <p className="text-xs text-muted-foreground">
                HTTPS、またはTailscale /
                SSHでローカルへ転送したURLだけを許可します。
              </p>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="token">アクセストークン</Label>
              <Input
                id="token"
                type="password"
                placeholder="GUI fixtureでは入力できません"
                autoComplete="off"
                disabled
              />
              <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
                <IconShieldLock className="size-3.5" />
                デモでは入力・保存しません。実装時はOSの資格情報ストアを使用します。
              </p>
            </div>
          </div>
        ) : (
          <Alert>
            <IconInfoCircle />
            <AlertTitle>単一バイナリ構成</AlertTitle>
            <AlertDescription>
              配布版は静的SPAを内包します。このGUI
              fixtureはローカルSolveを実行せず、将来Rustライブラリを同一プロセスで呼び出します。
            </AlertDescription>
          </Alert>
        )}

        {kind === "remote" ? (
          <Alert>
            <IconInfoCircle />
            <AlertTitle>今回の実装範囲</AlertTitle>
            <AlertDescription>
              プロファイル画面と通信契約だけを定義しています。接続テスト、認証、ジョブ送信はまだ行いません。
            </AlertDescription>
          </Alert>
        ) : null}

        <DialogFooter>
          <Button variant="ghost" onClick={() => handleOpenChange(false)}>
            キャンセル
          </Button>
          <Button onClick={applyProfile}>このデモのSolve先に設定</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
