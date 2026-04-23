#!/usr/bin/env bash
set -euo pipefail

# Installs a cargo binary from either crates.io or a GitHub repository,
# without permanently installing pkgx or Rust.
#
# Usage:
#   ./install-cargo-bin.sh --crate stardive
#   ./install-cargo-bin.sh --crate stardive --version 0.1.0
#   ./install-cargo-bin.sh --source github --repo stardive/stardive-api --crate stardive
#   ./install-cargo-bin.sh --source github --repo stardive/stardive-api --crate stardive --version 0.1.0
#   ./install-cargo-bin.sh --crate ripgrep --install-dir ~/.local/bin
#
# All binaries the crate produces are installed automatically.
#
# Flags:
#   --source <crate|github>  Source to install from         (default: crate)
#   --crate <name>           Crate name                     (required)
#   --repo <owner/repo>      GitHub repo in owner/repo form (required when --source github)
#   --version <x.y.z>        Version or git tag             (default: latest)
#   --install-dir <path>     Where to place the binary      (default: /usr/local/bin)
#
# Environment variable overrides:
#   TOOL_SOURCE, TOOL_CRATE, TOOL_REPO, TOOL_VERSION, TOOL_INSTALL_DIR

# ── defaults ────────────────────────────────────────────────────────
TOOL_SOURCE="${TOOL_SOURCE:-crate}"
TOOL_CRATE="${TOOL_CRATE:-}"
TOOL_REPO="${TOOL_REPO:-}"
TOOL_VERSION="${TOOL_VERSION:-}"
TOOL_INSTALL_DIR="${TOOL_INSTALL_DIR:-/usr/local/bin}"

# ── parse flags ─────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
    --source)
        TOOL_SOURCE="$2"
        shift 2
        ;;
    --crate)
        TOOL_CRATE="$2"
        shift 2
        ;;
    --repo)
        TOOL_REPO="$2"
        shift 2
        ;;
    --version)
        TOOL_VERSION="$2"
        shift 2
        ;;
    --install-dir)
        TOOL_INSTALL_DIR="$2"
        shift 2
        ;;
    -h | --help)
        sed -n '2,/^$/p' "$0" | sed 's/^# \?//'
        exit 0
        ;;
    *)
        echo "unknown flag: $1" >&2
        exit 1
        ;;
    esac
done

# ── validate ────────────────────────────────────────────────────────
if [[ -z "$TOOL_CRATE" ]]; then
    echo "error: --crate <name> is required" >&2
    exit 1
fi

if [[ "$TOOL_SOURCE" != "crate" && "$TOOL_SOURCE" != "github" ]]; then
    echo "error: --source must be 'crate' or 'github'" >&2
    exit 1
fi

if [[ "$TOOL_SOURCE" == "github" && -z "$TOOL_REPO" ]]; then
    echo "error: --repo <owner/repo> is required when --source github" >&2
    exit 1
fi

# ── bootstrap rust via pkgx if needed ───────────────────────────────
if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required" >&2
    exit 1
fi

if ! command -v install >/dev/null 2>&1; then
    echo "'install' command is required" >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found locally, bootstrapping via pkgx..." >&2
    if ! eval "$(sh <(curl -fsS https://pkgx.sh) +rust-lang.org +curl.se)"; then
        echo "failed to acquire temporary pkgx rust toolchain" >&2
        exit 1
    fi
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo unavailable after bootstrap" >&2
    exit 1
fi

# ── isolated build ──────────────────────────────────────────────────
WORKDIR="$(mktemp -d)"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

export CARGO_HOME="$WORKDIR/cargo-home"
export RUSTUP_HOME="$WORKDIR/rustup-home"
INSTALL_ROOT="$WORKDIR/install-root"

# ── build from the chosen source ────────────────────────────────────
case "$TOOL_SOURCE" in
crate)
    CRATE_SPEC="$TOOL_CRATE"
    if [[ -n "$TOOL_VERSION" ]]; then
        CRATE_SPEC="${TOOL_CRATE}@${TOOL_VERSION}"
    fi
    cargo install --locked --root "$INSTALL_ROOT" "$CRATE_SPEC"
    ;;
github)
    REPO_URL="https://github.com/${TOOL_REPO}.git"
    if [[ -n "$TOOL_VERSION" ]]; then
        TAG="$TOOL_VERSION"
        if [[ "$TAG" =~ ^[0-9]+ ]]; then
            TAG="v${TAG}"
        fi
        cargo install --locked --root "$INSTALL_ROOT" --git "$REPO_URL" --tag "$TAG" "$TOOL_CRATE"
    else
        cargo install --locked --root "$INSTALL_ROOT" --git "$REPO_URL" "$TOOL_CRATE"
    fi
    ;;
esac

# ── discover all produced binaries ──────────────────────────────────
BIN_DIR="$INSTALL_ROOT/bin"
if [[ ! -d "$BIN_DIR" ]] || [[ -z "$(ls -A "$BIN_DIR" 2>/dev/null)" ]]; then
    echo "build completed but no binaries were produced in $BIN_DIR" >&2
    exit 1
fi

mapfile -t BINS < <(find "$BIN_DIR" -maxdepth 1 -type f -executable)

if [[ ${#BINS[@]} -eq 0 ]]; then
    echo "build completed but no executables were found in $BIN_DIR" >&2
    exit 1
fi

# ── install all produced binaries ───────────────────────────────────
INSTALLED=()
for BIN_SRC in "${BINS[@]}"; do
    BIN_NAME="$(basename "$BIN_SRC")"
    TARGET_PATH="$TOOL_INSTALL_DIR/$BIN_NAME"

    if [[ -w "$TOOL_INSTALL_DIR" ]]; then
        install -m 0755 "$BIN_SRC" "$TARGET_PATH"
    else
        if ! command -v sudo >/dev/null 2>&1; then
            echo "need write access to $TOOL_INSTALL_DIR (try as root or install sudo)" >&2
            exit 1
        fi
        sudo install -m 0755 "$BIN_SRC" "$TARGET_PATH"
    fi

    INSTALLED+=("$TARGET_PATH")
done

echo ""
for P in "${INSTALLED[@]}"; do
    echo "installed $(basename "$P") → $P"
done
echo ""
echo "run: $(basename "${INSTALLED[0]}") --help"
