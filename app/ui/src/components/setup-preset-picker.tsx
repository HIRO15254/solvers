import { IconFlask, IconTrees } from "@tabler/icons-react"

import { Badge } from "@/components/ui/badge"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
} from "@/components/ui/card"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  RECOMMENDED_SETUP_PRESETS,
  TREE_PRESETS,
  type RecommendedSetupPresetId,
  type TreePresetId,
} from "@/lib/setup-config"

type SetupPresetPickerProps = {
  setupPreset: RecommendedSetupPresetId | "custom"
  treePreset: TreePresetId | "custom"
  onSetupPresetChange: (preset: RecommendedSetupPresetId) => void
  onTreePresetChange: (preset: TreePresetId) => void
}

export function SetupPresetPicker({
  setupPreset,
  treePreset,
  onSetupPresetChange,
  onTreePresetChange,
}: SetupPresetPickerProps) {
  const selectedSetup = RECOMMENDED_SETUP_PRESETS.find(
    (preset) => preset.id === setupPreset
  )
  const selectedTree = TREE_PRESETS.find((preset) => preset.id === treePreset)

  return (
    <Card size="sm">
      <CardHeader className="border-b">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="font-heading text-sm font-medium">
            推奨構成・Treeテンプレート
          </h2>
          <Badge variant="secondary">main · 2026-07-25</Badge>
        </div>
        <CardDescription>
          構成プリセットは全設定を置換し、Treeテンプレートは現在のtable・ICM・Solver設定を保持します。
        </CardDescription>
      </CardHeader>
      <CardContent className="preset-picker-grid">
        <div className="preset-picker-cell">
          <span className="preset-picker-icon">
            <IconFlask />
          </span>
          <div className="field-stack min-w-0">
            <div className="flex items-center justify-between gap-2">
              <Label>推奨構成</Label>
              {selectedSetup ? (
                <Badge variant="outline">{selectedSetup.status}</Badge>
              ) : null}
            </div>
            <Select
              value={setupPreset}
              onValueChange={(value) => {
                if (value && value !== "custom") {
                  onSetupPresetChange(value as RecommendedSetupPresetId)
                }
              }}
            >
              <SelectTrigger className="w-full" aria-label="推奨構成">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="custom">現在のカスタム設定</SelectItem>
                {RECOMMENDED_SETUP_PRESETS.map((preset) => (
                  <SelectItem value={preset.id} key={preset.id}>
                    {preset.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="field-help">
              {selectedSetup?.description ??
                "編集内容を維持しています。mainの暫定anchorを選ぶと全項目を置換します。"}
            </p>
          </div>
        </div>

        <div className="preset-picker-cell">
          <span className="preset-picker-icon">
            <IconTrees />
          </span>
          <div className="field-stack min-w-0">
            <Label>サンプルTree</Label>
            <Select
              value={treePreset}
              onValueChange={(value) => {
                if (value && value !== "custom") {
                  onTreePresetChange(value as TreePresetId)
                }
              }}
            >
              <SelectTrigger className="w-full" aria-label="サンプルTree">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {treePreset === "custom" ? (
                  <SelectItem value="custom">現在のカスタムTree</SelectItem>
                ) : null}
                {TREE_PRESETS.map((preset) => (
                  <SelectItem value={preset.id} key={preset.id}>
                    {preset.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="field-help">
              {selectedTree?.description ??
                "Treeを手動編集しています。テンプレートを選ぶとTree項目だけを置換します。"}
            </p>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
