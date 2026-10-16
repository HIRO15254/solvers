# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Release tags use `v` followed by the workspace version, for example `v0.1.0`.

## Compatibility during 0.x

The input schema, `solvers.nlh/v1`, is the compatibility unit. Binary artifact
formats (`.sol` solutions and checkpoints) may change in any 0.x minor release;
compatibility with older artifacts is not guaranteed. Unsupported older formats
are rejected with guidance to re-solve from a current `solvers.nlh/v1` config.
Version 0.1.0 accepts only P1 `.sol` format 4 and checkpoint format 5.
The human-readable `--version` includes a Git revision (`unknown` when unavailable);
run manifests keep `cliVersion` as the plain workspace semantic version.

## [0.1.0] - 2026-10-16

### Added

- P1 No-Limit Hold'em heads-up postflop solving from flop, turn, or river
  street starts, with ranges and a configurable betting tree supplied through
  `solvers.nlh/v1`. The solver uses exact combo-vector CFR without card
  abstraction and evaluates best responses in the specified finite game.
- Cash chipEV with optional rake and tournament ICM utility. Zero-sum games
  report exploitability; general-sum games report seat-specific best-response
  gains and NashConv without a zero-sum convergence guarantee.
- The `solvers` CLI for input creation and validation, solving and checkpoint
  resume, run monitoring, solution inspection, strategy/EV/range/equity export,
  solution comparison, and multi-board reports.
- The `solversd` job daemon with an HTTP API, bearer-token authentication,
  TLS support, and durable run-directory state.
- A Web GUI served by the daemon: P1 input setup, run progress and convergence,
  and node-by-node postflop result browsing.
- Version output with the workspace version and source Git revision; P1-only
  CLI builds identify themselves with `P1 only`.

### Distribution

- P1-only archives for Windows, Linux, and macOS containing `solvers`,
  `solversd`, and the built Web GUI. P2 Multiway Preflop and `derive` are
  excluded from these builds; P2 remains in the source repository as a
  provisional product whose method and quality guarantees are not yet decided.
- MIT OR Apache-2.0 licensing, bundled license notices, and SHA-256 checksums.
- Version 0.1.0 binaries are unsigned; Windows SmartScreen or macOS Gatekeeper
  may require the documented manual steps before execution. No installer is
  provided.
