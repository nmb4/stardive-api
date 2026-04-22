#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. install rust first: https://rustup.rs" >&2
  exit 1
fi

cargo install stardive --locked

echo "Installed stardive CLI. Run: stardive --help"
