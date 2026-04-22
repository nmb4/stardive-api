#!/usr/bin/env bash
set -euo pipefail

# Installs the `stardive` CLI without permanently installing pkgx or Rust.
# Optional env vars:
# - STARDIVE_VERSION (example: 0.1.0)
# - STARDIVE_INSTALL_DIR (default: /usr/local/bin)

STARDIVE_VERSION="${STARDIVE_VERSION:-}"
STARDIVE_INSTALL_DIR="${STARDIVE_INSTALL_DIR:-/usr/local/bin}"

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

if ! command -v install >/dev/null 2>&1; then
  echo "install command is required" >&2
  exit 1
fi

if ! eval "$(sh <(curl -fsS https://pkgx.sh) +rust-lang.org +curl.se)"; then
  echo "failed to acquire temporary pkgx rust toolchain" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo unavailable after pkgx bootstrap" >&2
  exit 1
fi

WORKDIR="$(mktemp -d)"
cleanup() {
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

export CARGO_HOME="$WORKDIR/cargo-home"
export RUSTUP_HOME="$WORKDIR/rustup-home"
INSTALL_ROOT="$WORKDIR/stardive-root"

CRATE_SPEC="stardive"
if [[ -n "$STARDIVE_VERSION" ]]; then
  CRATE_SPEC="stardive@${STARDIVE_VERSION}"
fi

cargo install --locked --root "$INSTALL_ROOT" "$CRATE_SPEC"

BIN_SRC="$INSTALL_ROOT/bin/stardive"
if [[ ! -x "$BIN_SRC" ]]; then
  echo "build completed but stardive binary was not produced" >&2
  exit 1
fi

TARGET_PATH="$STARDIVE_INSTALL_DIR/stardive"
if [[ -w "$STARDIVE_INSTALL_DIR" ]]; then
  install -m 0755 "$BIN_SRC" "$TARGET_PATH"
else
  if ! command -v sudo >/dev/null 2>&1; then
    echo "need write access to $STARDIVE_INSTALL_DIR (try as root or install sudo)" >&2
    exit 1
  fi
  sudo install -m 0755 "$BIN_SRC" "$TARGET_PATH"
fi

echo "installed stardive to $TARGET_PATH"
echo "run: stardive --help"
