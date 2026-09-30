#!/usr/bin/env bash
set -euo pipefail

# Scripts directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

VERSION="$(grep '^version = ' "${ROOT_DIR}/Cargo.toml" | head -n1 | cut -d '"' -f2)"
TARGET_ARCH="${1:-$(uname -m)}"
TARGET_OS="linux"

echo "==> Packaging CONCORD v${VERSION} for ${TARGET_OS}-${TARGET_ARCH}..."

DIST_DIR="${ROOT_DIR}/dist"
mkdir -p "${DIST_DIR}"

# Ensure release binary is compiled
echo "==> Building release binary..."
cargo build --release --manifest-path "${ROOT_DIR}/Cargo.toml" -p concord

RELEASE_BIN="${ROOT_DIR}/target/release/concord"
if [[ ! -f "${RELEASE_BIN}" ]]; then
    echo "Error: Release binary not found at ${RELEASE_BIN}" >&2
    exit 1
fi

TMP_STAGE="$(mktemp -d)"
trap 'rm -rf "${TMP_STAGE}"' EXIT

STAGE_DIR="${TMP_STAGE}/concord-v${VERSION}-${TARGET_OS}-${TARGET_ARCH}"
mkdir -p "${STAGE_DIR}"

cp "${RELEASE_BIN}" "${STAGE_DIR}/concord"
chmod +x "${STAGE_DIR}/concord"
cp "${ROOT_DIR}/README.md" "${STAGE_DIR}/README.md"
cp "${ROOT_DIR}/LICENSE" "${STAGE_DIR}/LICENSE"

ARCHIVE_NAME="concord-v${VERSION}-${TARGET_OS}-${TARGET_ARCH}.tar.gz"
ARCHIVE_PATH="${DIST_DIR}/${ARCHIVE_NAME}"

echo "==> Creating archive ${ARCHIVE_NAME}..."
tar -czf "${ARCHIVE_PATH}" -C "${TMP_STAGE}" "concord-v${VERSION}-${TARGET_OS}-${TARGET_ARCH}"

echo "==> Generating SHA256 checksums..."
cd "${DIST_DIR}"
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${ARCHIVE_NAME}" > "${ARCHIVE_NAME}.sha256"
elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${ARCHIVE_NAME}" > "${ARCHIVE_NAME}.sha256"
fi

cat "${ARCHIVE_NAME}.sha256"
echo "==> Packaging complete: ${ARCHIVE_PATH}"
