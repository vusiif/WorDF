#!/usr/bin/env bash
# WorDF automated release packaging script for POSIX systems (Linux / macOS).
# Builds optimized release binaries, bundles documentation, and computes SHA-256 checksums.
# License: GPL-3.0-or-later

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${PROJECT_ROOT}"

VERSION=$(grep -m1 '^version\s*=' Cargo.toml | sed -E 's/version\s*=\s*"([^"]+)"/\1/')
DIST_DIR="${PROJECT_ROOT}/dist"
OS_NAME="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH_NAME="$(uname -m)"

rm -rf "${DIST_DIR}"
mkdir -p "${DIST_DIR}"

echo "=================================================="
echo " WorDF Release Packager - v${VERSION} (${OS_NAME}-${ARCH_NAME})"
echo "=================================================="

package_flavor() {
    local flavor="$1"
    local flags="$2"
    local suffix="$3"

    echo ""
    echo ">> Building [${flavor}] flavor: cargo build --release ${flags}..."
    if [ -n "${flags}" ]; then
        cargo build --release ${flags}
    else
        cargo build --release
    fi

    local bin_path="${PROJECT_ROOT}/target/release/wordf"
    if [ ! -f "${bin_path}" ]; then
        echo "Error: Release binary not found at ${bin_path}" >&2
        exit 1
    fi

    local stage_dir="${DIST_DIR}/stage_${flavor}"
    mkdir -p "${stage_dir}"

    cp "${bin_path}" "${stage_dir}/wordf"
    chmod +x "${stage_dir}/wordf"
    cp "${PROJECT_ROOT}/README.md" "${stage_dir}/README.md"
    cp "${PROJECT_ROOT}/LICENSE" "${stage_dir}/LICENSE"

    local archive_name="wordf-${suffix}-v${VERSION}-${OS_NAME}-${ARCH_NAME}.tar.gz"
    local archive_path="${DIST_DIR}/${archive_name}"

    echo ">> Creating archive: ${archive_name}..."
    tar -czf "${archive_path}" -C "${stage_dir}" wordf README.md LICENSE
    rm -rf "${stage_dir}"

    local hash=""
    if command -v sha256sum >/dev/null 2>&1; then
        hash=$(sha256sum "${archive_path}" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
        hash=$(shasum -a 256 "${archive_path}" | awk '{print $1}')
    else
        hash="checksum-tool-missing"
    fi

    echo ">> Package ready: ${archive_name} SHA256: ${hash}"
    echo "${hash}  ${archive_name}" >> "${DIST_DIR}/SHA256SUMS.txt"
}

FLAVOR="${1:-all}"

if [ "${FLAVOR}" = "all" ] || [ "${FLAVOR}" = "default" ]; then
    package_flavor "default" "" "full"
fi

if [ "${FLAVOR}" = "all" ] || [ "${FLAVOR}" = "lite" ]; then
    package_flavor "lite" "--no-default-features" "lite"
fi

echo ""
echo "=================================================="
echo " Distribution packages generated in: ${DIST_DIR}"
ls -la "${DIST_DIR}"
echo ""
echo "SHA256SUMS:"
cat "${DIST_DIR}/SHA256SUMS.txt"
echo "=================================================="
