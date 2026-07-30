import { useMemo, useState } from "react"
import { IconLoader2, IconPlayerPlay } from "@tabler/icons-react"

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
import type { NativeResumeOverrides } from "@/lib/native-gateway"

export type ResumeSubmission = {
  name: string
  overrides: NativeResumeOverrides
}

type ResumeDialogProps = {
  sourceLabel: string
  completedSweeps: string
  suggestedName: string
  busy: boolean
  onCancel: () => void
  onConfirm: (submission: ResumeSubmission) => void
}

type ResumeDraft = {
  name: string
  maxSweeps: string
  maxTime: string
  stopTarget: string
  evaluationSamples: string
  checkEverySweeps: string
  checkpointInterval: string
  threads: string
  memory: string
}

function suggestedMaxSweeps(completedSweeps: string) {
  try {
    return (BigInt(completedSweeps) + 1_000_000n).toString()
  } catch {
    return ""
  }
}

function optional(
  target: NativeResumeOverrides,
  key: keyof NativeResumeOverrides,
  value: string
) {
  const normalized = value.trim()
  if (normalized) {
    target[key] = normalized
  }
}

export function ResumeDialog({
  sourceLabel,
  completedSweeps,
  suggestedName,
  busy,
  onCancel,
  onConfirm,
}: ResumeDialogProps) {
  const [draft, setDraft] = useState<ResumeDraft>(() => ({
    name: suggestedName,
    maxSweeps: suggestedMaxSweeps(completedSweeps),
    maxTime: "",
    stopTarget: "",
    evaluationSamples: "",
    checkEverySweeps: "",
    checkpointInterval: "",
    threads: "",
    memory: "",
  }))

  const maxSweepsError = useMemo(() => {
    const token = draft.maxSweeps.trim()
    if (!/^[0-9]+$/.test(token)) {
      return "再開後のsweep上限を10進整数で入力してください。"
    }
    try {
      return BigInt(token) > BigInt(completedSweeps)
        ? null
        : `completed sweeps (${completedSweeps}) より大きい値が必要です。`
    } catch {
      return "sweep値を比較できません。"
    }
  }, [completedSweeps, draft.maxSweeps])

  const update = (key: keyof ResumeDraft, value: string) => {
    setDraft((current) => ({ ...current, [key]: value }))
  }

  const submit = () => {
    if (maxSweepsError) {
      return
    }
    const overrides: NativeResumeOverrides = {
      maxSweeps: draft.maxSweeps.trim(),
    }
    optional(overrides, "maxTime", draft.maxTime)
    optional(overrides, "stopTarget", draft.stopTarget)
    optional(overrides, "evaluationSamples", draft.evaluationSamples)
    optional(overrides, "checkEverySweeps", draft.checkEverySweeps)
    optional(overrides, "checkpointInterval", draft.checkpointInterval)
    optional(overrides, "threads", draft.threads)
    optional(overrides, "memory", draft.memory)
    onConfirm({
      name: draft.name.trim() || suggestedName,
      overrides,
    })
  }

  return (
    <Dialog open onOpenChange={(open) => !open && !busy && onCancel()}>
      <DialogContent className="gap-5 p-5 sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>CheckpointからSolveを再開</DialogTitle>
          <DialogDescription>
            {sourceLabel} · completed {completedSweeps}{" "}
            sweeps。既存のcheckpointは変更せず、新しいjobを作成します。
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 sm:grid-cols-2">
          <div className="field-stack sm:col-span-2">
            <Label htmlFor="resume-name">Solve名</Label>
            <Input
              id="resume-name"
              value={draft.name}
              onChange={(event) => update("name", event.target.value)}
            />
          </div>
          <div className="field-stack sm:col-span-2">
            <Label htmlFor="resume-max-sweeps">
              再開後のsweep上限 <span aria-hidden="true">*</span>
            </Label>
            <Input
              id="resume-max-sweeps"
              inputMode="numeric"
              value={draft.maxSweeps}
              aria-invalid={Boolean(maxSweepsError)}
              aria-describedby="resume-max-sweeps-help"
              onChange={(event) => update("maxSweeps", event.target.value)}
            />
            <p
              id="resume-max-sweeps-help"
              className={
                maxSweepsError
                  ? "text-xs text-destructive"
                  : "text-xs text-muted-foreground"
              }
            >
              {maxSweepsError ??
                "checkpointのcompleted sweepsより大きい絶対上限です。"}
            </p>
          </div>
          <div className="field-stack">
            <Label htmlFor="resume-max-time">最大時間（任意）</Label>
            <Input
              id="resume-max-time"
              placeholder="例: 12h"
              value={draft.maxTime}
              onChange={(event) => update("maxTime", event.target.value)}
            />
          </div>
          <div className="field-stack">
            <Label htmlFor="resume-memory">Memory budget（任意）</Label>
            <Input
              id="resume-memory"
              placeholder="例: 32GiB"
              value={draft.memory}
              onChange={(event) => update("memory", event.target.value)}
            />
          </div>
          <div className="field-stack">
            <Label htmlFor="resume-stop-target">Stop target（任意）</Label>
            <Input
              id="resume-stop-target"
              inputMode="decimal"
              value={draft.stopTarget}
              onChange={(event) => update("stopTarget", event.target.value)}
            />
          </div>
          <div className="field-stack">
            <Label htmlFor="resume-threads">Threads（任意）</Label>
            <Input
              id="resume-threads"
              inputMode="numeric"
              placeholder="例: 16"
              value={draft.threads}
              onChange={(event) => update("threads", event.target.value)}
            />
          </div>
          <div className="field-stack">
            <Label htmlFor="resume-evaluation-samples">
              Evaluation samples（任意）
            </Label>
            <Input
              id="resume-evaluation-samples"
              inputMode="numeric"
              value={draft.evaluationSamples}
              onChange={(event) =>
                update("evaluationSamples", event.target.value)
              }
            />
          </div>
          <div className="field-stack">
            <Label htmlFor="resume-check-every">
              Check every sweeps（任意）
            </Label>
            <Input
              id="resume-check-every"
              inputMode="numeric"
              value={draft.checkEverySweeps}
              onChange={(event) =>
                update("checkEverySweeps", event.target.value)
              }
            />
          </div>
          <div className="field-stack sm:col-span-2">
            <Label htmlFor="resume-checkpoint-interval">
              Checkpoint interval（任意）
            </Label>
            <Input
              id="resume-checkpoint-interval"
              placeholder="例: 30m"
              value={draft.checkpointInterval}
              onChange={(event) =>
                update("checkpointInterval", event.target.value)
              }
            />
          </div>
        </div>

        <DialogFooter>
          <Button variant="ghost" disabled={busy} onClick={onCancel}>
            キャンセル
          </Button>
          <Button disabled={busy || Boolean(maxSweepsError)} onClick={submit}>
            {busy ? (
              <IconLoader2 className="animate-spin" />
            ) : (
              <IconPlayerPlay />
            )}
            新しいjobを開始
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
