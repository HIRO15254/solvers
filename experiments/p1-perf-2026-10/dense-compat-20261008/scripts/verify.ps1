$env:CARGO_BUILD_JOBS = '1'
$env:CARGO_INCREMENTAL = '0'
cargo fmt --all --check *> runs/t24/fmt.log
$formatExitCode = $LASTEXITCODE
$formatExitCode | Set-Content runs/t24/fmt.exit
if ($formatExitCode -ne 0) { Get-Content runs/t24/fmt.log -Tail 30; exit $formatExitCode }

cargo clippy --workspace --all-targets -- -D warnings *> runs/t24/clippy.log
$clippyExitCode = $LASTEXITCODE
$clippyExitCode | Set-Content runs/t24/clippy.exit
if ($clippyExitCode -ne 0) { Get-Content runs/t24/clippy.log -Tail 45; exit $clippyExitCode }

cargo test --workspace -- --test-threads=1 --nocapture *> runs/t24/tests.log
$testsExitCode = $LASTEXITCODE
$testsExitCode | Set-Content runs/t24/tests.exit
if ($testsExitCode -ne 0) { Get-Content runs/t24/tests.log -Tail 60; exit $testsExitCode }

python tools/check_docs.py *> runs/t24/check-docs.log
$docsExitCode = $LASTEXITCODE
$docsExitCode | Set-Content runs/t24/check-docs.exit
Get-Content runs/t24/tests.log -Tail 18
Get-Content runs/t24/check-docs.log
exit $docsExitCode
