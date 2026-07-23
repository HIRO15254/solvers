# Solvers Web GUI v1 仕様

Status: **操作可能な visual fixture shell 実装済み / Solve transport 未実装**  
確定日: 2026-07-23  
UI preset: `shadcn/ui --preset bdvw9nmi`

この文書は、Web 技術で作成する Solvers GUI v1 の画面、配布形態、Local /
Remote Solve 境界に対する正本である。「今回」と「target」を次の意味で区別する。

- **今回**: `app/ui` の操作可能な visual fixture shell と、静的 SPA を内包する
  `app/desktop` の Tauri shell。fixture から solver、bridge、filesystem、
  credential store、remote network への接続はない。
- **target**: 本書で確定する製品契約。実装済みという意味ではない。

GUI v1 が設定・実行する solver は **Multiway Preflop v1**
(`solvers.multiway-preflop/v1`)だけである。この solver は preflop から river
までを内部で解くため、結果 explorer では preflop hand class と postflop
abstraction bucket の両方を扱う。HU preflop (`kind = "preflop"`) と exact
postflop (`kind = "postflop"`) はアプリ全体および CLI には残るが、GUI v1 の
設定対象ではない。対応を追加するときは GUI contract の新しい version として定義する。

## 0. 実装境界

| 項目 | 今回 | target |
|---|---|---|
| Setup / Solving / Results | 操作可能な visual shell | 実データと全 target 項目を接続 |
| form control | fixture の一部だけ操作可能 | v1 config 全体と lossless に同期 |
| strategy | synthetic 13×13 fixture | atomic live average / final artifact |
| node / actor 選択 | visual fixture | node ごとの実データへ接続 |
| Local / Remote profile | 選択 UI | LocalGateway / RemoteGateway |
| validation / preflight | synthetic 表示 | v1 parser / normalizer / resource preflight |
| Solve / cancel / resume | 未実装 | durable job contract |
| artifact import / export | 未実装 | CLI と同じ reader / writer |
| Tauri shell | 静的 SPA を raw executable に内包 | Local solver library も同じ executable に link |

今回の画面に表示する run、validation、resource、strategy、result、artifact はすべて
fixture であり、実行・検証済みの値ではない。fixture build では全画面に
`FIXTURE — Solve は実行されません` を常時表示し、Local / Remote の primary
action はそれぞれ `ローカル実行をプレビュー` / `リモート実行をプレビュー` とする。
書込・停止・共有・export を装う action は disabled にし、理由を表示する。

## 1. 配布形態

### 1.1 単一 executable

GUI は platform / architecture ごとの raw executable `solvers-gui` 1個で起動する。
React / Vite の `frontendDist` は Tauri 2 が compile 時に executable へ内包する。
実行時に次を要求しない。

- Node.js / package manager
- 別 Web server
- 外部ブラウザー
- `solvers` CLI sidecar
- application 独自の別 daemon

Tauri が利用する OS WebView は許容する。macOS は WebKit、Windows は WebView2、
Linux は WebKitGTK を OS prerequisite とし、これらを application sidecar とは
数えない。raw executable は build した OS / architecture 専用であり、1個の
cross-platform binary を意味しない。

現段階で確認する成果物は `cargo build --release -p solvers-desktop` が生成する
`target/release/solvers-gui` である。`bundle.active = false` のため、署名済み
`.app`、MSI、AppImage、notarization、installer、release CI は今回の完成主張に
含めない。将来 platform 標準の配布 envelope を追加しても、install 後に必要な
application executable は `solvers-gui` だけとし、CLI sidecar を追加しない。

```text
solvers-gui executable
├── Tauri 2 native window + OS WebView
├── embedded React / Vite static SPA
└── SolverGateway
    ├── Fixture data      今回: screen が直接参照
    ├── LocalGateway      target: 同一 process の Rust library
    └── RemoteGateway     target: Tauri Rust HTTP client
```

standalone CLI `solvers` は研究・batch 運用向けの別成果物として維持する。ただし
GUI の起動と Local Solve はその存在に依存しない。LocalGateway は `app/cli` の
library 境界を link し、CLI と同じ v1 parser、normalizer、solver entrypoint、
artifact writer を使う。

SPA route は server fallback を必要としない hash route とする。

- `#/setup`
- `#/solve/{jobId}`
- `#/results/{jobId}`

## 2. 共通 shell

- 左側に `設定`、`Solve中`、`結果` の workflow navigation を置く。
- 上部に run、status、connection profile、theme 切替を置く。
- profile は `Local` または `Remote`。job 作成後の
  `connectionProfileId` は immutable とし、別 profile へ暗黙移動しない。
- status は `COMPLETED` のような曖昧な総称で置換せず、正式 status を表示する。
- 推奨 window は 1440×900、最小 window は 960×640。
- 960px 未満では navigation を横一列、main content を1 column にする。
- matrix / bucket table だけ horizontal scroll を許し、page 全体には発生させない。

visual system は preset から生成された次を維持する。

- style: Mira
- base / theme: Zinc
- font: Inter
- icons: Tabler
- radius: Small
- menu accent: Bold

## 3. Setup

### 3.1 flow

1. preset または TOML を選ぶ。
2. table / player を編集する。
3. betting tree、economics、solver、runtime を編集する。
4. Local / Remote profile を選ぶ。
5. profile 上で validation / resource preflight を実行する。
6. effective config と guarantee boundary を確認する。
7. `Solve を開始` する。

フォームと TOML を別の正本として持たない。編集 draft は v1 schema の field と
lossless に対応し、BB、rate、target、duration、memory、整数を含む入力 token を
**文字列のまま**保持する。`.001 BB` 判定や numeric conversion に JavaScript
`number` を使わない。Rust normalizer が成功した後だけ、effective TOML、fixed-unit
integer、resource estimate を表示する。unknown field、irrelevant field、
`.001 BB` で表せない値は warning ではなく error とする。

### 3.2 target 項目

基本領域:

- Solve 名
- preset
- `game.seat_count`、`game.button`
- `standard_blinds`、`preflop_first_to_act`、`common_ante_bb`
- default stack / range
- seat ごとの stack、range、blind、ante
- tree frontend (`standard | script`)
- economics (`cash | tournament-icm`) と rake
- max sweeps、stop target、resources

詳細領域:

- typed tree rules / `.mwtree`
- abstraction kind、street bucket、opponent bucket
- `rollouts_per_state`。既定は検証済みの 512
- information recall
- solver seed、exploration、batch、discount、pruning
- evaluation budget / cadence、checkpoint interval
- output probability encoding

現 schema で無効な field は disabled control として残さず、form、TOML、request
から除外する。economics により stop target の unit が変わるため、cash では
BB/hand、tournament ICM では prize unit を明示する。

### 3.3 `.mwtree` と Remote

Local draft は config file からの相対 `.mwtree` を読み込める。Remote request には
client filesystem path や source file を送らない。Tauri 側の v1 normalizer が
`.mwtree` を canonical typed rules へ compile し、`kind = "standard"` の
reparse 可能な effective TOML へ materialize してから server へ送る。server は
受け取った effective TOML を同じ v1 parser で再検証し、client が送った config
fingerprint と一致しなければ job を作らない。Remote v1 に file upload endpoint は
設けない。

### 3.4 summary

- connection profile と、Remote の場合は verified server identity
- threads / peak memory / abstraction / rough time estimate
- schema / range / tree / resource validation
- guarantee boundary
- Local run directory または Remote managed run label

time estimate は sweep budget を消化する参考値であり、収束時刻の予測ではない。

## 4. Solving

### 4.1 progress

Multiway で表示する正式指標:

- sweeps / max sweeps
- elapsed time
- memory、traversals / second、hand updates / second、infosets
- seat EV / CI
- average positive regret
- strategy drift
- measured deviation の one-sided 95% CI upper bound
- checkpoint availability / age

3人以上に `NashConv`、`exploitability`、`GTO`、`converged` を使わない。
`target-reached` だけを品質目標到達として扱う。`sweep-limit` と `time-limit` は
正常停止だが品質目標到達ではない。progress percentage は max sweeps に対する
消化率であり、収束確率ではない。Solving 画面に ETA を表示する場合は
`sweep budget 基準の参考時間` と明記し、収束 ETA と呼ばない。

### 4.2 live strategy

strategy は desktop で常時見える主要領域に置く。正式表示は、最後に完了した
sweep 境界までの **Linear average strategy** だけである。last iterate や現在の
regret-matched strategy を正式 profile として表示しない。

header に必ず次を表示する。

- `LIVE AVERAGE | STALE | FINAL`
- snapshot revision と取得時刻
- `as of {sweeps} sweeps`
- actor seat、street、public-history breadcrumb
- connection loss 時は最後に server から取得した時刻

preflop decision node は対角 pair、上三角 suited、下三角 offsuit の 13×13
matrix にする。セルは server が返す typed legal actions を stacked color で表示し、
選択時に exact percentage、EV availability、combo count を表示する。action 数を
3つに固定しない。

flop / turn / river decision node は abstraction bucket view に切り替える。bucket ID、
recall を使う場合は bucket path、reach weight、visited status、全 legal action
比率を table / grid で表示する。postflop bucket strategy を 13×13 hand class
strategy に見せかけない。将来 hand projection を追加する場合は、projection method
と reach weighting を別 schema で定義する。

未訪問 entry は `unvisited` とし、probability、EV、action bar を表示しない。
uniform、0%、synthetic mixture で補完せず、accessible name も
`{label}: unvisited` とする。snapshot 更新中は前回値を残し、画面全体を
skeleton に戻さない。

## 5. Results

### 5.1 summary

- exact terminal status
- sweeps、elapsed、finished time、Solve profile
- measured deviation / target と unit
- evaluation samples と CI
- visited strategy coverage
- approximation notice

terminal status は次だけを使う。

- `target-reached`
- `sweep-limit`
- `time-limit`
- `cancelled`
- `resource-limit`
- `failed`

`target-reached` は success color を使える。`sweep-limit` / `time-limit` は neutral、
`cancelled` / `resource-limit` は warning、`failed` は error とする。
`sweep-limit` を緑の `COMPLETED` badge で表示しない。

### 5.2 views

- Strategy explorer: public tree、node、actor、preflop 13×13 または postflop bucket
- Seat EV: mean と 95% CI
- Quality: evaluation history、regret、drift、deviation
- Effective config: defaults / derived value を含む reparse 可能 TOML
- Artifacts: `run.json`、`progress.jsonl`、`solution.mwsol`、
  `checkpoint.mwckpt`

`cancelled` / `resource-limit` / `failed` は正式 solution を持たない。result summary
と checkpoint availability は表示できるが、存在しない solution / export action
は disabled にし理由を表示する。

## 6. UI 内部契約

### 6.1 gateway boundary

React screen は HTTP、Bearer token、Tauri command、local path を直接扱わない。
`SolveGateway` を唯一の I/O 境界とする。今回の `app/ui/src/lib/solve-contract.ts`
には fixture view model と target adapter DTO の compile-time sketch を併置する。
field 名と型の正本は本節と Remote v3 contract とし、transport 実装時はこの契約へ
一致させる。

```ts
interface SolveGateway {
  getCapabilities(profileId: string): Promise<BridgeCapabilitiesV3>
  validate(
    profileId: string,
    draft: MultiwayV1Draft
  ): Promise<ValidationResult>
  createJob(
    profileId: string,
    request: CreateJobRequest
  ): Promise<JobSnapshot>
  listJobs(
    profileId: string,
    cursor?: string
  ): Promise<JobPage>
  getJob(profileId: string, jobId: string): Promise<JobSnapshot>
  getEvents(
    profileId: string,
    jobId: string,
    afterSequence?: string
  ): Promise<JobEventPage>
  getResult(profileId: string, jobId: string): Promise<JobResult>
  getStrategy(
    profileId: string,
    jobId: string,
    nodeId: string
  ): Promise<StrategySnapshotV1>
  exportArtifact(
    profileId: string,
    jobId: string,
    request: ExportRequest
  ): Promise<ExportReceipt>
  cancelJob(profileId: string, jobId: string): Promise<JobSnapshot>
  resumeJob(
    profileId: string,
    jobId: string,
    request: ResumeJobRequest
  ): Promise<JobSnapshot>
}
```

RemoteGateway は SSE を優先し、切断時に `getEvents` polling へ切り替える。
LocalGateway は同じ DTO と state machine を返し、screen に transport 分岐を
持ち込まない。

### 6.2 draft と validated config

`MultiwayV1Draft` は `solvers.multiway-preflop/v1` の全 field を lossless に表す。
user-entered numeric token は `DecimalString | IntegerString | DurationString |
MemoryString` とし、validation 前に number へ変換しない。`ValidationResult` は次を返す。

wire DTO の `DecimalString` は exponent を使わない有限10進数、
`IntegerString` / `UInt64String` は base-10 integer の canonical string とする。
leading `+`、不要な leading zero、`NaN`、infinity、whitespace を許さない。
validation 前の draft token は user input を lossless に保ち、validation 後の
effective config / DTO だけを canonical form にする。

- `valid`
- path 付き error (`code`, `path`, `message`)
- normalized / default-expanded `effectiveConfigToml`
- `configFingerprint`
- resource preflight
- guarantee boundary と units

warning は performance hint のみに使い、schema、irrelevant field、precision、
range、tree、resource hard limit は error とする。

```ts
type ValidationResult = {
  valid: boolean
  errors: Array<{ code: string; path: string; message: string }>
  warnings: Array<{ code: string; path: string; message: string }>
  effectiveConfigToml: string | null
  configFingerprint: string | null
  preflight: {
    threads: string
    peakMemoryBytes: string
    abstraction: string
  } | null
  guaranteeBoundary: string
  units: { utility: string; chipUnitBb: "0.001" }
}
```

```ts
type CreateJobRequest = {
  idempotencyKey: string
  name: string
  schema: "solvers.multiway-preflop/v1"
  effectiveConfigToml: string
  configFingerprint: string
}

type ResumeJobRequest = {
  idempotencyKey: string
  name: string
  overrides: {
    maxSweeps?: IntegerString
    maxTime?: DurationString
    stopTarget?: DecimalString | "default"
    evaluationSamples?: IntegerString
    checkEverySweeps?: IntegerString
    checkpointInterval?: DurationString
    threads?: IntegerString | "auto"
    memory?: MemoryString | "auto"
  }
}

type ExportRequest =
  | { kind: "artifact"; artifact: "run" | "progress" | "solution" | "checkpoint" }
  | {
      kind: "view"
      view: "node" | "seats" | "quality"
      format: "json" | "csv"
      nodeId?: string
    }
```

Resume の変更可能 field はこの `overrides` だけとする。server は checkpoint の
effective config へ override を適用し、table / range / tree / economics /
abstraction / recall / solver fingerprint が変わる request を error にする。

## 7. Remote Solve v3 contract

この節は target contract であり、現行 bridge `/v2` の実装済み機能ではない。
すべての `/v3` endpoint は Bearer authentication を要求する。

### 7.1 ConnectionProfile、credential、URL

```ts
type ConnectionProfile =
  | {
      id: string
      kind: "local"
      name: string
    }
  | {
      id: string
      kind: "remote"
      name: string
      baseUrl: string
      credentialRef: string
      serverId: string
    }
```

Remote profile 作成時だけ token を password field へ入力する。React は token を
state、localStorage、TOML、log、result に保存しない。Tauri Rust command が直接
受け取り、OS credential store へ保存して opaque な `credentialRef` を返す。
profile 削除時は対応 credential も削除する。

server credential は 32 random bytes (256 bit)以上とし、server の protected
credential file に保存する。Unix は mode 0600、Windows は実行 user だけの ACL
を要求する。通常の `serve` log、health、error に token を出さず、operator が
明示的に作成した時だけ一度表示して out-of-band で共有する。token は明示 rotation
まで有効で、rotation は旧 token を即時失効させる。job は停止しないが、client は
新 token の再登録が必要になる。v3 は1つの token に server 上の全 job の
read/create/cancel/resume 権限を与え、multi-user ACL は対象外とする。

Remote base URL は path、query、fragment、userinfo を持たない exact origin とし、
次だけを許可する。

- `https://...`
- SSH port forwarding 等で client loopback へ転送した
  `http://127.0.0.1:<port>`、`http://localhost:<port>`、
  `http://[::1]:<port>`

非 loopback の平文 `http://` は、Tailscale address を含め拒否する。Tailscale を
使う場合は Tailscale Serve 等で HTTPS termination する。GUI は tunnel、VPN、
reverse proxy、certificate を構築しない。Remote machine の operator は
`solvers serve` に加え、loopback bridge までの encrypted tunnel / TLS termination
を用意してから URL と token を渡す。HTTPS は OS trust store で検証し、
certificate error の bypass、self-signed certificate の自動承認、HTTP downgrade
を提供しない。

Remote request は WebView `fetch` ではなく Tauri Rust HTTP client が送る。
有効 token を持ち `Origin` がない非 browser request は proxy 後の syntactically
valid な Host を許可する。`Origin` を持つ browser request は server に設定した
exact Origin / Host の両方を要求する。bridge 自体は loopback bind を維持し、
`X-Forwarded-*` を authentication に使わない。

初回 handshake で取得した stable `serverId` を profile に保存する。同じ URL から
別 `serverId` が返った場合は接続を拒否し、user が profile を明示的に置換するまで
job を送らない。

### 7.2 capability handshake

`GET /v3/health` は次の形を返す。

```json
{
  "service": "solvers",
  "serverId": "018f4d2e-2f9f-7f61-a6f4-4b45d5342d32",
  "solverVersion": "0.1.0",
  "apiVersion": 3,
  "busy": false,
  "configSchemas": ["solvers.multiway-preflop/v1"],
  "artifactSchemas": {
    "runJson": [3],
    "progressJsonl": [3],
    "mwsol": [4],
    "mwckpt": [7],
    "strategySnapshot": [1]
  },
  "features": {
    "liveStrategySnapshots": true,
    "resume": true,
    "durableJobs": true,
    "eventTransports": ["sse", "poll"]
  },
  "retention": {
    "events": "job-lifetime",
    "jobs": "operator-managed"
  },
  "limits": {
    "maxPlayers": 9,
    "maxConcurrentJobs": 1,
    "memoryBytes": "68719476736"
  }
}
```

`serverId` は server data directory に永続化する UUID であり、process restart で
変えない。service、API、config schema、`mwsol=4`、`mwckpt=7`、必要 feature が
一致しない場合は Start / Resume を拒否し、不足項目を列挙する。

### 7.3 common response と job state

error は全 endpoint で次へ統一する。

```json
{
  "error": {
    "code": "idempotency_conflict",
    "message": "The key was already used with a different request.",
    "retryable": false,
    "details": {}
  }
}
```

job state は次の union とする。

```ts
type JobState =
  | "queued"
  | "validating"
  | "running"
  | "cancelling"
  | "target-reached"
  | "sweep-limit"
  | "time-limit"
  | "cancelled"
  | "resource-limit"
  | "failed"
```

JSON field は camelCase、timestamp は UTC RFC 3339、chip amount は
`milliBb` integer string とする。byte / sweep / sequence / revision と、
EV、rate、ratio、metric、elapsed seconds は decimal string にする。seat index、
street count、combo count、0〜65,535 の fixed-point probability のように上限が
schema で小さく固定された値だけを JSON number にする。

```ts
type UInt64String = string
type IntegerString = string
type DecimalString = string

type Estimate = {
  mean: DecimalString
  stderr: DecimalString
  ci95: [DecimalString, DecimalString]
}

type MultiwayProgressV3 = {
  sweeps: UInt64String
  maxSweeps: UInt64String
  elapsedSecs: DecimalString
  stopTarget: DecimalString
  stopTargetUnit: string
  memoryBytes: UInt64String
  traversalsPerSecond: DecimalString
  handUpdatesPerSecond: DecimalString
  infosets: UInt64String
  checkpoint: {
    available: boolean
    generatedAt: string | null
  }
  seats: Array<{
    seat: number
    profileEv: Estimate | null
    averagePositiveRegret: DecimalString
    strategyDriftL1: DecimalString
    deviationGain: Estimate | null
  }>
}

type ArtifactDescriptor = {
  available: boolean
  byteLength: UInt64String | null
  sha256: string | null
}

type JobSnapshot = {
  id: string
  state: JobState
  createdAt: string
  startedAt: string | null
  finishedAt: string | null
  configFingerprint: string
  resumedFromJobId: string | null
  progress: MultiwayProgressV3 | null
  resumeAvailable: boolean
  artifacts: {
    run: ArtifactDescriptor
    progress: ArtifactDescriptor
    solution: ArtifactDescriptor
    checkpoint: ArtifactDescriptor
  }
  error: {
    code: string
    message: string
    retryable: boolean
  } | null
}

type JobEvent = {
  sequence: UInt64String
  jobId: string
  generatedAt: string
  kind: "state" | "progress" | "checkpoint" | "resource-warning"
  state: JobState
  progress: MultiwayProgressV3 | null
}

type JobEventPage = {
  events: JobEvent[]
  lastSequence: UInt64String | null
  terminal: boolean
}

type JobResult = {
  job: JobSnapshot
  terminalStatus:
    | "target-reached"
    | "sweep-limit"
    | "time-limit"
    | "cancelled"
    | "resource-limit"
    | "failed"
  effectiveConfigToml: string
  guaranteeBoundary: string
  units: { utility: string; chipUnitBb: "0.001" }
}

type JobPage = {
  items: JobSnapshot[]
  nextCursor: string | null
}

type ExportReceipt = {
  fileName: string
  byteLength: UInt64String
  sha256: string
}
```

`connectionProfileId` は server DTO へ送らず、SolverGateway が client-local binding
として `JobSnapshot` と一緒に保持する。

### 7.4 endpoints

| Method | Path | 契約 |
|---|---|---|
| GET | `/v3/health` | capability / identity |
| POST | `/v3/validate` | effective v1 TOML の再検証と preflight |
| POST | `/v3/jobs` | managed job 作成 |
| GET | `/v3/jobs?cursor=&limit=` | 新しい順の job page |
| GET | `/v3/jobs/{id}` | status / latest progress |
| GET | `/v3/jobs/{id}/events` | JSON polling または SSE |
| GET | `/v3/jobs/{id}/result` | terminal run summary |
| GET | `/v3/jobs/{id}/strategy-snapshots/latest?nodeId=` | live / final strategy |
| GET | `/v3/jobs/{id}/artifacts/{kind}` | `run`, `progress`, `solution`, `checkpoint` |
| POST | `/v3/jobs/{id}/export` | server-side JSON / CSV view |
| POST | `/v3/jobs/{id}/cancel` | cooperative cancel |
| POST | `/v3/jobs/{id}/resume` | checkpoint から child job 作成 |

validate body は `schema`, `effectiveConfigToml`, `configFingerprint` を持ち、
response は §6.2 の `ValidationResult` と同じ形にする。server が再計算した
fingerprint が request と違う場合は HTTP 409 `config_fingerprint_mismatch` とする。

`GET /v3/jobs` の `limit` は既定 50、最大 200。cursor は opaque とする。
artifact / export bytes は Tauri Rust 側で受け、native save dialog で選んだ client
path へ atomic write する。client path を HTTP request に含めず、React には
保存結果と表示用 filename だけを返す。artifact response は `Content-Length`,
`Content-Disposition` と lowercase hex SHA-256 の
`X-Content-SHA256` を返し、Gateway は
保存完了前に size と digest を検証する。

`POST /v3/jobs` と `POST .../resume` は `Idempotency-Key` header を必須とする。
値は client が生成する 128 bit 以上、1〜128文字の printable ASCII とする。
scope は credential + HTTP method + canonical path。server は key、request body
hash、最初の response を job と同じ期間 durable に保存する。同じ key / 同じ
body は元 response を返し、同じ key / 異なる body は HTTP 409
`idempotency_conflict` とする。

Gateway の `CreateJobRequest.idempotencyKey` /
`ResumeJobRequest.idempotencyKey` は HTTP header へ移す。create body は `name`,
`schema`, `effectiveConfigToml`, `configFingerprint` だけを持ち、resume body は
`name`, `overrides` だけを持つ。client path を受けない。server は1 job 1 managed
run directoryを作る。
同時実行上限を超えた job は FIFO の `queued` とし、重複 create を busy error で
再試行させない。

cancel は idempotent とする。queued は直ちに `cancelled`、running は cooperative
cancel へ移行し、
terminal job では既存 snapshot を HTTP 200 で返す。resume は complete checkpoint
を持つ terminal job にだけ許可し、新しい job ID と `resumedFromJobId` を持つ
child job を作る。元 job と artifact は変更しない。

### 7.5 progress: SSE と polling

server は job ごとに 0 から始まる monotonic `sequence` を全 event に付け、
`progress.jsonl` へ append / flush してから配信する。event は job directory が
存在する限り prune / compact しない。

同じ endpoint を content negotiation する。

- `Accept: text/event-stream`: SSE。`id` は sequence、`event` は event kind、
  `data` は同じ JSON DTO。15秒ごとに comment keepalive を送り、terminal event
  送信後に close する。
- `Accept: application/json`: polling page。`afterSequence` は exclusive、
  省略時は先頭から、`limit` は既定 200 / 最大 1000。response は
  `events`, `lastSequence`, `terminal` を返す。

SSE reconnect は `Last-Event-ID`、polling は `afterSequence` から再開する。
RemoteGateway は SSE を優先し、接続できなければ2秒 polling に切り替える。
transport failure は 1, 2, 4, 8, 16, 30秒の capped exponential backoff で再接続する。
job directory が存在する間は sequence gap を許さない。

### 7.6 durability、restart、retention

v3 server は operator が指定した persistent data directory を使い、tempdir と
in-memory job map だけに依存しない。`run.json`、`progress.jsonl`、
`solution.mwsol`、`checkpoint.mwckpt` と idempotency record を job directory に
保存する。run / solution / checkpoint は temp file + flush + atomic rename、
progress は event 単位で append + flush する。

server restart 時は data directory から job index を再構築する。restart 前に
`queued` / `validating` / `running` / `cancelling` だった job は自動再開せず、
`failed`、`error.code = "server-restarted"` とし、complete checkpoint があれば
`resumeAvailable = true` にする。user の Resume は前節の child job を作る。

active job と terminal job は自動削除しない。event と idempotency record は
job directory と同じ lifetime を持つ。v3 に delete endpoint は設けない。
operator が out-of-band で directory を削除した後は job 全体を HTTP 404 とする。

### 7.7 StrategySnapshot v1

```ts
type TypedAction = {
  id: string
  semantic: "fold" | "check" | "call" | "bet-to" | "raise-to"
  amountMilliBb: IntegerString | null
  allIn: boolean
  fullRaise: boolean | null
  label: string
}

type StrategyEntry = {
  id: string
  label: string
  status: "visited" | "unvisited"
  weight: DecimalString
  comboCount: number | null
  bucketPath: number[] | null
  probabilityU16: number[] | null
  ev: { value: DecimalString; unit: string } | null
}

type StrategySnapshotV1 = {
  schemaVersion: 1
  jobId: string
  revision: UInt64String
  status: "live-average" | "stale" | "final"
  strategyKind: "linear-average"
  generatedAt: string
  asOfSweeps: string
  currentSweeps: string
  node: {
    nodeId: string
    actorSeat: number
    street: "preflop" | "flop" | "turn" | "river"
    potMilliBb: IntegerString
    activeOpponents: number
    breadcrumb: Array<{
      actorSeat: number
      action: TypedAction
    }>
  }
  actions: TypedAction[]
  view:
    | { kind: "preflop-hand-classes"; entries: StrategyEntry[] }
    | { kind: "postflop-buckets"; entries: StrategyEntry[] }
  approximate: true
  coverage: DecimalString
}
```

`nodeId` は game fingerprint 内で stable な opaque public-history ID であり、
breadcrumb label を query key に使わない。visited entry の `probabilityU16` は
actions と同じ順序・同じ長さで、各値 0〜65,535、合計 65,535 とする。
unvisited entry は `probabilityU16 = null`, `ev = null`。coverage は
requested view の全 entry weight に対する visited entry weight の比で 0〜1 とする。

server は `run.stop.check_every_sweeps` の各 complete evaluation boundary と、
terminal 正常停止の直前に immutable average snapshot を publish する。同じ
snapshot に異なる sweep の block を混在させない。

- `live-average`: running job の最新 scheduled boundary を publish 済み。
- `stale`: 最新 scheduled boundary の publish に失敗し、前回 snapshot を返した。
- `final`: `target-reached` / `sweep-limit` / `time-limit` の formal solution と一致。

snapshot がまだ1件もなければ HTTP 409 `snapshot_not_available_yet`。
`cancelled` / `resource-limit` / `failed` は final snapshot を生成せず、取得済みの
前回 snapshot を `stale` として表示できるだけとする。

### 7.8 connection loss と window close

- connection loss 時は `Connection lost`、last received time、cached strategy を
  `STALE` として表示し、Remote job が停止したと推測しない。
- `jobId + connectionProfileId` と last sequence を local app data に保存し、
  GUI restart 後に handshake、job status、event replay の順で reattach する。
- Remote job は GUI を閉じても継続する。close 前に binding / last sequence を
  保存し、solver 側へ cancel を送らない。
- Local job は GUI process 内で動くため background continuation を提供しない。
  window close を一度保留し、`停止して checkpoint を保存` と
  `window に戻る` の2択を出す。前者は partial sweep を破棄し、最後の complete
  sweep 境界で checkpoint を atomic write してから終了する。checkpoint が失敗した
  場合は window を閉じず error を表示する。

## 8. 現行 bridge `/v2` との差分

現行 `/v2` は health、validate、create / status / result、cancel、checkpoint、
完了後 strategy endpoint を持つが、GUI v1 の正式 transport ではない。

v3 実装で必要な差分:

1. v1 parser / normalizer を validate と create へ接続する。
2. health を API v3、config v1、run/progress v3、`.mwsol` v4、
   `.mwckpt` v7、snapshot v1へ更新する。
3. active job の atomic average strategy snapshot を追加する。
4. durable sequence event、SSE、polling replay を追加する。
5. tempdir / in-memory job map を persistent managed run に置換する。
6. durable idempotency と child resume を追加する。
7. authenticated Tauri Rust client の Origin なし request を許可する。
8. status、error、JSON naming、artifact availability を本書へ正規化する。

これらが全部実装されるまで、GUI は現行 bridge を compatible / connected と
表示してはならない。exact postflop job API は GUI v1 の scope 外とする。

## 9. Accessibility / responsive

target は WCAG 2.2 AA を基準とする。

- landmark、skip link、visible label、focus ring
- status を色だけで伝えない
- matrix cell は hand と全 action percentage、unvisited cell は unvisited を
  accessible name に含める
- keyboard で cell / bucket を選択し、arrow / Home / End の roving tabindex
- chart と同値の numeric summary
- live update は throttled `aria-live="polite"`
- `prefers-reduced-motion` で pulse / transition 停止
- 200% zoom で primary action と重要 status を維持
- 1366×768、1024×768、960×640 を最低確認対象とする

## 10. 受入条件

### 10.1 visual fixture shell

- `pnpm lint` と `pnpm build` が成功する。
- `cargo build --release -p solvers-desktop` が raw `solvers-gui` を生成する。
- executable 1個の起動で、network / Node.js /別 Web server なしに SPA を表示する。
- hash URL と navigation から3画面を移動できる。
- Local / Remote profile UI を切り替えられる。
- 全画面で fixture と分かり、実 Solve / validation / artifact write を装わない。
- Solving に preflop strategy fixture が主要領域として見える。

### 10.2 product transport

- LocalGateway が同じ executable 内の v1 libraryだけで validate / solve / cancel /
  resume / result / artifact を完結する。
- RemoteGateway が v3 handshake、credential store、durable job、SSE / polling
  replay、reattach を満たす。
- Local / Remote は同じ effective config fingerprint、job state、result semantics
  を返す。
- live strategy は revision、status、as-of sweeps、street、node ID を持ち、
  preflop 13×13 / postflop bucket view を正しく切り替える。
- unvisited entry は probability / EV を持たず、formal solution と混同しない。
- Results は exact terminal status と guarantee boundary を表示し、存在しない
  artifact action を無効化する。
- Remote integration test は duplicate create、SSE reconnect、polling replay、
  server restart、resume、token rotation、artifact checksum を含む。
