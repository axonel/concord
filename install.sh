#!/bin/sh
# CONCORD Installer
# One-command installer for CONCORD: Development Environment Engine
# Repository: https://github.com/axonel/concord

set -eu

REPO="${CONCORD_REPO:-axonel/concord}"
DEFAULT_VERSION="v0.2.1"
INSTALL_DIR="${CONCORD_INSTALL_DIR:-$HOME/.local/bin}"

# ANSI color codes
BOLD='\033[1m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
RESET='\033[0m'

print_banner() {
    printf "${CYAN}${BOLD}"
    printf "CONCORD — Development Environment Resolution Engine\n"
    printf "──────────────────────────────────────────────────\n"
    printf "${RESET}"
}

info() {
    printf "${GREEN}==>${RESET} ${BOLD}%s${RESET}\n" "$1"
}

warn() {
    printf "${YELLOW}warning:${RESET} %s\n" "$1"
}

error() {
    printf "${RED}error:${RESET} %s\n" "$1" >&2
    exit 1
}

# 1. Platform Detection
detect_os() {
    OS="$(uname -s)"
    case "$OS" in
        Linux|linux)
            echo "linux"
            ;;
        Darwin|darwin)
            error "macOS is not yet supported in this release. CONCORD currently targets Linux development environments."
            ;;
        *)
            error "Unsupported operating system: $OS. CONCORD requires Linux."
            ;;
    esac
}

# 2. Architecture Detection
detect_arch() {
    ARCH="$(uname -m)"
    case "$ARCH" in
        x86_64|amd64)
            echo "x86_64"
            ;;
        aarch64|arm64)
            error "ARM64 architecture support will be available in future releases. Supported architecture: x86_64."
            ;;
        *)
            error "Unsupported CPU architecture: $ARCH. Supported architecture: x86_64."
            ;;
    esac
}

# 3. HTTP Download Helper (curl or wget)
download_file() {
    url="$1"
    output="$2"

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$output"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$output" "$url"
    else
        error "Neither curl nor wget was found on your system. Please install one to proceed."
    fi
}

# 4. Resolve Target Version
resolve_version() {
    if [ -n "${CONCORD_VERSION:-}" ]; then
        echo "$CONCORD_VERSION"
        return
    fi

    LATEST=""
    if command -v curl >/dev/null 2>&1; then
        LATEST=$(curl -fsSL --connect-timeout 2 -m 4 "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name":' | head -n 1 | sed -E 's/.*"([^"]+)".*/\1/' || true)
    elif command -v wget >/dev/null 2>&1; then
        LATEST=$(wget -qO- --timeout=4 "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name":' | head -n 1 | sed -E 's/.*"([^"]+)".*/\1/' || true)
    fi

    if [ -n "$LATEST" ]; then
        echo "$LATEST"
    else
        echo "$DEFAULT_VERSION"
    fi
}

# 5. Compute SHA256
compute_sha256() {
    file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$file" | awk '{print $NF}'
    else
        error "No SHA256 utility found (sha256sum, shasum, or openssl required)."
    fi
}

# 6. Check System Prerequisites
check_prerequisites() {
    if ! command -v tar >/dev/null 2>&1; then
        error "'tar' utility is required but not found in PATH."
    fi
    if ! command -v gzip >/dev/null 2>&1; then
        error "'gzip' utility is required but not found in PATH."
    fi
    if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
        error "Either 'curl' or 'wget' is required but neither was found in PATH."
    fi
    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1 && ! command -v openssl >/dev/null 2>&1; then
        error "A checksum tool ('sha256sum', 'shasum', or 'openssl') is required but none was found in PATH."
    fi
}

main() {
    print_banner
    check_prerequisites

    OS=$(detect_os)
    ARCH=$(detect_arch)
    VERSION=$(resolve_version)

    info "Detected platform: ${OS}-${ARCH}"
    info "Target version:   ${VERSION}"
    info "Install directory: ${INSTALL_DIR}"

    ARCHIVE_NAME="concord-${VERSION}-${OS}-${ARCH}.tar.gz"
    BASE_URL="${CONCORD_BASE_URL:-https://github.com/${REPO}/releases/download/${VERSION}}"
    RELEASE_URL="${BASE_URL}/${ARCHIVE_NAME}"
    CHECKSUM_URL="${RELEASE_URL}.sha256"

    TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'concord-install')"
    trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

    ARCHIVE_PATH="${TMP_DIR}/${ARCHIVE_NAME}"
    CHECKSUM_PATH="${TMP_DIR}/${ARCHIVE_NAME}.sha256"

    info "Downloading ${ARCHIVE_NAME}..."
    if ! download_file "$RELEASE_URL" "$ARCHIVE_PATH"; then
        error "Failed to download release archive from $RELEASE_URL"
    fi

    info "Downloading checksum..."
    if ! download_file "$CHECKSUM_URL" "$CHECKSUM_PATH"; then
        error "Failed to download checksum file from $CHECKSUM_URL"
    fi

    # Read expected checksum (first field of .sha256 file)
    EXPECTED_HASH="$(awk '{print $1}' "$CHECKSUM_PATH")"
    ACTUAL_HASH="$(compute_sha256 "$ARCHIVE_PATH")"

    info "Verifying SHA256 checksum..."
    if [ "$EXPECTED_HASH" != "$ACTUAL_HASH" ]; then
        error "Checksum verification failed!\n  Expected: $EXPECTED_HASH\n  Actual:   $ACTUAL_HASH"
    fi
    printf "  Checksum OK: %s\n" "$ACTUAL_HASH"

    info "Extracting binary..."
    tar -xzf "$ARCHIVE_PATH" -C "$TMP_DIR"

    EXTRACTED_BIN="${TMP_DIR}/concord-${VERSION}-${OS}-${ARCH}/concord"
    if [ ! -f "$EXTRACTED_BIN" ]; then
        # Fallback if tar extracted directly to root
        EXTRACTED_BIN="${TMP_DIR}/concord"
    fi

    if [ ! -f "$EXTRACTED_BIN" ]; then
        error "Extracted archive did not contain 'concord' binary."
    fi

    # Verify write permissions and install atomically
    mkdir -p "$INSTALL_DIR" 2>/dev/null || error "Cannot create installation directory '$INSTALL_DIR'. Check permissions."
    if [ ! -w "$INSTALL_DIR" ]; then
        error "Installation directory '$INSTALL_DIR' is not writable. Check permissions or configure CONCORD_INSTALL_DIR."
    fi

    TMP_INSTALL_FILE="${INSTALL_DIR}/.concord.tmp.$$"
    cp "$EXTRACTED_BIN" "$TMP_INSTALL_FILE"
    chmod +x "$TMP_INSTALL_FILE"
    mv -f "$TMP_INSTALL_FILE" "${INSTALL_DIR}/concord"

    info "Successfully installed concord to ${INSTALL_DIR}/concord"

    # Verify execution
    INSTALLED_VERSION="$("${INSTALL_DIR}/concord" --version 2>/dev/null || true)"
    if [ -z "$INSTALLED_VERSION" ]; then
        error "Binary installed but failed to execute: ${INSTALL_DIR}/concord --version"
    fi

    printf "${GREEN}${BOLD}✓ Verified: %s${RESET}\n\n" "$INSTALLED_VERSION"

    # Check if INSTALL_DIR is in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*)
            # Already on PATH
            ;;
        *)
            warn "${INSTALL_DIR} is NOT currently in your PATH environment variable."
            printf "\nTo make 'concord' available from any directory, add this to your shell profile:\n"
            printf "  ${BOLD}export PATH=\"%s:\$PATH\"${RESET}\n\n" "$INSTALL_DIR"
            printf "For bash (~/.bashrc):\n"
            printf "  echo 'export PATH=\"%s:\$PATH\"' >> ~/.bashrc && source ~/.bashrc\n\n" "$INSTALL_DIR"
            printf "For zsh (~/.zshrc):\n"
            printf "  echo 'export PATH=\"%s:\$PATH\"' >> ~/.zshrc && source ~/.zshrc\n\n" "$INSTALL_DIR"
            printf "For fish (~/.config/fish/config.fish):\n"
            printf "  fish_add_path %s\n\n" "$INSTALL_DIR"
            ;;
    esac

    printf "Run CONCORD on your repository:\n"
    printf "  ${CYAN}${BOLD}concord .${RESET}\n"
}

main "$@"
