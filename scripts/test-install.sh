#!/usr/bin/env bash
# Clean-Room Installation Smoke Test for CONCORD
# Verifies installation, default paths, exit codes, and execution without developer environment dependencies.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

BOLD='\033[1m'
GREEN='\033[0;32m'
RED='\033[0;31m'
CYAN='\033[0;36m'
RESET='\033[0m'

pass() {
    printf "${GREEN}✓ PASS:${RESET} %s\n" "$1"
}

fail() {
    printf "${RED}✗ FAIL:${RESET} %s\n" "$1" >&2
    exit 1
}

step() {
    printf "\n${CYAN}${BOLD}==> %s${RESET}\n" "$1"
}

# 1. Setup isolated clean-room environment
TMP_TEST_DIR="$(mktemp -d -t concord-clean-test.XXXXXX)"
trap 'rm -rf "$TMP_TEST_DIR"' EXIT INT TERM

MOCK_HOME="${TMP_TEST_DIR}/mock_home"
mkdir -p "$MOCK_HOME"

step "Test 1: Default Clean-Room Installation into \$HOME/.local/bin"

# Run install.sh with HOME pointing to MOCK_HOME and minimal PATH
# Notice: CONCORD_INSTALL_DIR is NOT set, testing default behavior
(
    export HOME="$MOCK_HOME"
    export PATH="/usr/bin:/bin:/usr/sbin:/sbin"
    cd "$ROOT_DIR"
    sh install.sh
)

INSTALLED_BIN="${MOCK_HOME}/.local/bin/concord"

if [[ ! -f "$INSTALLED_BIN" ]]; then
    fail "Binary not found at expected default path: ${INSTALLED_BIN}"
fi

if [[ ! -x "$INSTALLED_BIN" ]]; then
    fail "Binary at ${INSTALLED_BIN} is not executable"
fi

pass "Default installation to \$HOME/.local/bin/concord succeeded"

step "Test 2: Execution and Version Verification"
INSTALLED_VER="$("$INSTALLED_BIN" --version)"
if [[ -z "$INSTALLED_VER" ]]; then
    fail "concord --version produced empty output"
fi
pass "Installed binary executed successfully: ${INSTALLED_VER}"

step "Test 3: CLI Exit Code Verification"

# 3a. Healthy project -> exit 0
set +e
"$INSTALLED_BIN" "${ROOT_DIR}/tests/fixtures/healthy-node-app" > /dev/null 2>&1
CODE_HEALTHY=$?
set -e
if [[ "$CODE_HEALTHY" -ne 0 ]]; then
    fail "Expected exit code 0 on healthy-node-app, got ${CODE_HEALTHY}"
fi
pass "Exit code 0 on compatible project verified"

# 3b. Broken project -> exit 1
set +e
"$INSTALLED_BIN" "${ROOT_DIR}/tests/fixtures/broken-node-version" > /dev/null 2>&1
CODE_BROKEN=$?
set -e
if [[ "$CODE_BROKEN" -ne 1 ]]; then
    fail "Expected exit code 1 on broken-node-version, got ${CODE_BROKEN}"
fi
pass "Exit code 1 on incompatible project verified"

# 3c. Invalid path -> exit 2
set +e
"$INSTALLED_BIN" "/path/that/does/not/exist/999" > /dev/null 2>&1
CODE_INVALID=$?
set -e
if [[ "$CODE_INVALID" -ne 2 ]]; then
    fail "Expected exit code 2 on nonexistent path, got ${CODE_INVALID}"
fi
pass "Exit code 2 on invalid path verified"

step "Test 4: JSON Output Contract Verification"

JSON_OUTPUT="$("$INSTALLED_BIN" "${ROOT_DIR}/tests/fixtures/healthy-node-app" --json)"
if ! echo "$JSON_OUTPUT" | jq . > /dev/null 2>&1; then
    fail "concord --json did not produce valid JSON on healthy project"
fi
pass "Valid JSON produced on healthy project"

JSON_ERROR="$("$INSTALLED_BIN" "/path/that/does/not/exist/999" --json 2>/dev/null || true)"
if ! echo "$JSON_ERROR" | jq -e '.error' > /dev/null 2>&1; then
    fail "concord /does/not/exist --json did not produce valid JSON with .error field"
fi
pass "Valid JSON error contract verified on missing path"

step "Test 5: Custom Install Directory Override"
CUSTOM_DIR="${TMP_TEST_DIR}/custom_prefix/bin"
(
    export HOME="$MOCK_HOME"
    export CONCORD_INSTALL_DIR="$CUSTOM_DIR"
    cd "$ROOT_DIR"
    sh install.sh
)

if [[ ! -x "${CUSTOM_DIR}/concord" ]]; then
    fail "Binary not found in custom directory: ${CUSTOM_DIR}/concord"
fi
pass "Custom installation directory override verified"

step "All Clean-Room Installation Tests PASSED Successfully!"
