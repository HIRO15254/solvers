# VM19 regional CPU quota review

Review date: 2026-09-27. Read-only review of existing receipts and primary documentation; no new cloud API calls, resource changes, or native execution.

The documented E2 CPU quota pool is **CPUS**, shared with N1. The current regional response's **E2_CPUS = 24 is not sufficient evidence that a 32-vCPU E2 launch is blocked**. This review does not describe E2_CPUS as deprecated, legacy, or unused: the official source does not establish that claim.

## Primary specification and limits

The [Compute Engine allocation quota documentation](https://docs.cloud.google.com/compute/resource-usage#cpu_quota), read on the review date, explicitly maps E2 to CPUS. Its [preemptible quota section](https://docs.cloud.google.com/compute/resource-usage#preemptible_quotas) says Spot VMs use standard quota when the project has no preemptible quota and has never requested it; after requesting preemptible quota, applicable resources can only consume that separate pool. Therefore PREEMPTIBLE_CPUS = 0 alone does not establish the project's request history or effective pool.

The same document explains that some projects also have a global CPUs (All Regions) quota, and that quota does not guarantee zonal resource availability. The reviewed VM19 response is regional only. This is a quota interpretation and historical-success check, **not confirmation that VM19 can start with 32 vCPUs now**. Any authorized launch must still be checked for actual success or a quota/capacity error; this review does not authorize a fallback, new quota request, deadline extension, or extra spend.

## Local evidence

Project: `solvers-abstraction-20260723`; region: `us-central1`; prior successful zone: `us-central1-b`.

VM19's successful regional query ran from `2026-09-27T08:19:30.571774+00:00` to `08:19:33.422733+00:00`. It reports CPUS limit 200 / usage 0, E2_CPUS limit 24 / usage 0, and PREEMPTIBLE_CPUS limit 0 / usage 0. Its complete stdout is byte-identical to VM18's successful regional query at `07:06:50.896495+00:00` to `07:06:54.095844+00:00`.

VM18 then successfully changed to `e2-highcpu-32` at `07:26:51.473366+00:00` to `07:26:57.324554+00:00`, and started at `07:27:02.189530+00:00` to `07:27:29.007283+00:00`. The subsequent describe receipt at `07:27:34.965733+00:00` to `07:27:37.843344+00:00` identifies instance **3769585733775752220**, name `solvers-r1-20260927-18`, machine `e2-highcpu-32`, provisioning model **SPOT**, and status **RUNNING** in that project/zone. All five command receipts exit 0 and bind their stdout/stderr hashes. These observations support standard-pool use at that time, but do not directly measure which quota counter was charged or prove current availability.

Paths below are relative to this file. SHA-256 pins bind the existing evidence without modifying it.

| Evidence | Bytes | SHA-256 |
| --- | ---: | --- |
| [VM19 regional stdout](preflight-quota01.stdout.log) | 12068 | `b8794f4470295c72590ce4e5295d0da15a98a540e8b104d4c13423b34c322e8e` |
| [VM19 regional receipt](preflight-quota01.result.json) | 650 | `2f34b91c97840ab3ac357111c628af472d7e772de5e6e9c16a27a96b08d5c0ce` |
| [VM18 regional stdout](../vm18/preflight-region01.stdout.log) | 12068 | `b8794f4470295c72590ce4e5295d0da15a98a540e8b104d4c13423b34c322e8e` |
| [VM18 regional receipt](../vm18/preflight-region01.result.json) | 650 | `979b5126da29d31072779f38624eca89c33faae6077d67dec3ecee431d638971` |
| [VM18 resize receipt](../vm18/resize01.result.json) | 713 | `0b7336ffd5d2e68e49859efb6ece6aab686442a683189fca9f19d4e637fdd856` |
| [VM18 start receipt](../vm18/start32-01.result.json) | 665 | `1f55554e5d0a98223cc211c40da33b132d1769e43e8cbf78571e51979c4d53ca` |
| [VM18 RUNNING stdout](../vm18/state32-01.stdout.log) | 545 | `c6af1cc26a9884dff32fa9acb77380ba7d81e832aaa8bebfa8bc441648636660` |
| [VM18 describe receipt](../vm18/state32-01.result.json) | 748 | `0cad01e057bc350dc2193253376345db2a2452f7fa8a7d0bbb2867c2d2b7d582` |
