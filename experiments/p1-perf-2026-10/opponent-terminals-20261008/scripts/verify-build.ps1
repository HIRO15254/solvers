$ErrorActionPreference = 'Stop'
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_INCREMENTAL='0'
Copy-Item -LiteralPath runs/t21/tests.log -Destination runs/t21/tests-before-lookup.log
cargo fmt --all
if ($LASTEXITCODE -ne 0) { throw 'fmt failed' }
cargo fmt --all --check *> runs/t21/fmt.log
if ($LASTEXITCODE -ne 0) { throw 'fmt check failed' }
cargo clippy --workspace --all-targets -- -D warnings *> runs/t21/clippy.log
if ($LASTEXITCODE -ne 0) { throw 'clippy failed' }
Write-Output 'Final fmt and clippy passed'
cargo test --workspace *> runs/t21/tests.log
if ($LASTEXITCODE -ne 0) { throw 'workspace tests failed' }
Write-Output 'Final workspace tests passed'
python tools/check_docs.py *> runs/t21/check-docs.log
if ($LASTEXITCODE -ne 0) { throw 'docs failed' }
cargo build -p cli --manifest-path runs/t21/head-source/Cargo.toml --target-dir target *> runs/t21/cli-head-build.log
if ($LASTEXITCODE -ne 0) { throw 'HEAD CLI build failed' }
Copy-Item -LiteralPath target/debug/solvers.exe -Destination runs/t21/solvers-head.exe
Write-Output 'HEAD CLI saved'
Remove-Item -LiteralPath target/debug/deps/libhu_engine-f23b7ac14f89fb7a.rlib,target/debug/deps/libhu_postflop-0a0bd83669cb77d0.rlib,target/debug/deps/libcli-92f26ffa6baea7bf.rlib,target/debug/solvers.exe,target/debug/deps/solvers.exe
cargo build -p cli --verbose *> runs/t21/cli-new-build.log
if ($LASTEXITCODE -ne 0) { throw 'new CLI build failed' }
Copy-Item -LiteralPath target/debug/solvers.exe -Destination runs/t21/solvers-new.exe
Write-Output 'New CLI saved'
cargo bench -p hu-postflop --bench kernels --no-run *> runs/t21/bench-new-build.log
if ($LASTEXITCODE -ne 0) { throw 'final benchmark build failed' }
$t21Scratch='C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad'
Copy-Item -LiteralPath target/release/deps/kernels-a9d9dc014dc65b87.exe -Destination (Join-Path $t21Scratch 't21-bench-new.exe')
Write-Output 'Final benchmark saved'
