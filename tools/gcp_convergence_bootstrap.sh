#!/usr/bin/env bash
# Prepare a fresh Ubuntu 24.04 Spot VM for a later convergence run.
# This script installs/builds only; it never starts the benchmark.
set -Eeuo pipefail

BASE_DIR=/opt/solvers-experiment
ARCHIVE=${BASE_DIR}/source.tgz
SOURCE_DIR=${BASE_DIR}/source
RESULTS_DIR=${BASE_DIR}/results
LOCK_DIR=${BASE_DIR}/.bootstrap.lock
RUST_VERSION=1.97.0
RUSTUP_INSTALL_URL=https://sh.rustup.rs
RUN_ID=$(date -u +%Y%m%dT%H%M%SZ)
LOG_FILE=${RESULTS_DIR}/bootstrap-${RUN_ID}.log

mkdir -p "${BASE_DIR}" "${RESULTS_DIR}"

fail() {
    echo "bootstrap error: $*" >&2
    printf 'failed_at=%s\nexit_code=1\nreason=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" \
        > "${RESULTS_DIR}/bootstrap-failed-${RUN_ID}.txt" || true
    exit 1
}

on_error() {
    local rc=$?
    printf 'failed_at=%s\nexit_code=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "${rc}" \
        > "${RESULTS_DIR}/bootstrap-failed-${RUN_ID}.txt" || true
    exit "${rc}"
}
trap on_error ERR

# mkdir is atomic and does not require a writable /var lock directory.
if ! mkdir "${LOCK_DIR}" 2>/dev/null; then
    fail "another bootstrap is already running (${LOCK_DIR})"
fi
trap 'rmdir "${LOCK_DIR}" 2>/dev/null || true' EXIT

# Keep the complete apt/rustup/cargo transcript in the result directory.
exec > >(tee -a "${LOG_FILE}") 2>&1
echo "bootstrap_started=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "run_id=${RUN_ID}"

if [[ -f "${RESULTS_DIR}/bootstrap-complete.txt" ]]; then
    echo "bootstrap already completed; nothing to do"
    exit 0
fi

[[ "$(id -u)" -eq 0 ]] || fail "must run as root"
[[ -f /etc/os-release ]] || fail "/etc/os-release is missing"
grep -qE '^ID=ubuntu$' /etc/os-release || fail "Ubuntu is required"
grep -qE '^VERSION_ID="24\.04"$' /etc/os-release || fail "Ubuntu 24.04 is required"
[[ -f "${ARCHIVE}" ]] || fail "source archive is missing: ${ARCHIVE}"

echo '== installing build prerequisites =='
if ! DEBIAN_FRONTEND=noninteractive apt-get update; then
    fail "apt-get update failed"
fi
if ! DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    build-essential ca-certificates curl; then
    fail "apt prerequisite installation failed"
fi

archive_sha=$(sha256sum "${ARCHIVE}" | awk '{print $1}')
echo "source_archive_sha256=${archive_sha}"

if [[ -e "${SOURCE_DIR}" ]]; then
    [[ -f "${SOURCE_DIR}/Cargo.toml" ]] || fail "existing source is not a Rust workspace: ${SOURCE_DIR}"
    source_marker="${SOURCE_DIR}/.source-archive.sha256"
    [[ -f "${source_marker}" ]] || fail "existing source has no archive hash marker: ${source_marker}"
    source_sha=$(tr -d '[:space:]' < "${source_marker}")
    [[ "${source_sha}" == "${archive_sha}" ]] || fail "existing source archive hash differs; refusing overwrite"
    echo "source exists; refusing to overwrite: ${SOURCE_DIR}"
else
    echo '== extracting source archive atomically =='
    staging_dir=$(mktemp -d "${BASE_DIR}/.source-staging.XXXXXX")
    if ! tar --extract --gzip --no-same-owner --file "${ARCHIVE}" --directory "${staging_dir}"; then
        rm -rf "${staging_dir}"
        fail "source archive extraction failed"
    fi
    if [[ ! -f "${staging_dir}/Cargo.toml" ]]; then
        rm -rf "${staging_dir}"
        fail "source archive must contain Cargo.toml at its root"
    fi
    printf '%s\n' "${archive_sha}" > "${staging_dir}/.source-archive.sha256"
    mv "${staging_dir}" "${SOURCE_DIR}"
    trap on_error ERR
    echo "source_extracted=${SOURCE_DIR}"
fi

export CARGO_HOME=${CARGO_HOME:-/root/.cargo}
export RUSTUP_HOME=${RUSTUP_HOME:-/root/.rustup}
export PATH="${CARGO_HOME}/bin:${PATH}"

echo '== installing pinned Rust toolchain =='
if ! command -v rustup >/dev/null 2>&1; then
    if ! curl --proto '=https' --tlsv1.2 --fail --silent --show-error "${RUSTUP_INSTALL_URL}" \
        | sh -s -- -y --profile minimal --default-toolchain none; then
        fail "rustup installation failed"
    fi
fi
if ! rustup toolchain list | awk '{print $1}' | grep -q "^${RUST_VERSION}-"; then
    if ! rustup toolchain install "${RUST_VERSION}" --profile minimal --no-self-update; then
        fail "Rust ${RUST_VERSION} installation failed"
    fi
fi
if ! rustup default "${RUST_VERSION}"; then
    fail "could not select Rust ${RUST_VERSION}"
fi
rustc --version --verbose
cargo --version

echo '== building cli and full-policy audit release binaries =='
if ! (cd "${SOURCE_DIR}" && cargo build --locked --release -p cli --bin solvers --example mw_checkpoint_audit); then
    fail "cargo build --locked --release -p cli --bin solvers --example mw_checkpoint_audit failed"
fi

BINARY=${SOURCE_DIR}/target/release/solvers
[[ -x "${BINARY}" ]] || fail "release binary was not produced: ${BINARY}"
AUDIT_BINARY=${SOURCE_DIR}/target/release/examples/mw_checkpoint_audit
[[ -x "${AUDIT_BINARY}" ]] || fail "audit binary was not produced: ${AUDIT_BINARY}"

ENV_FILE=${RESULTS_DIR}/environment-${RUN_ID}.txt
{
    echo "captured_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "run_id=${RUN_ID}"
    echo "source_dir=${SOURCE_DIR}"
    echo "source_archive=${ARCHIVE}"
    echo "source_archive_sha256=${archive_sha}"
    uname -a
    cat /etc/os-release
    rustc --version --verbose
    cargo --version
    echo "rust_toolchain=${RUST_VERSION}"
} > "${ENV_FILE}"

HASH_FILE=${RESULTS_DIR}/binary-sha256-${RUN_ID}.txt
sha256sum "${BINARY}" "${AUDIT_BINARY}" > "${HASH_FILE}"
printf 'binary=%s\n' "${BINARY}" >> "${HASH_FILE}"

cat > "${RESULTS_DIR}/bootstrap-complete.txt" <<EOF
completed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
run_id=${RUN_ID}
source=${SOURCE_DIR}
binary=${BINARY}
binary_sha256=$(sha256sum "${BINARY}" | awk '{print $1}')
audit_binary=${AUDIT_BINARY}
audit_binary_sha256=$(sha256sum "${AUDIT_BINARY}" | awk '{print $1}')
rust_toolchain=${RUST_VERSION}
EOF
echo "bootstrap completed; log=${LOG_FILE}"
