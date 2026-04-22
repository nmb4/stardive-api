#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

VERSION="${1:-}"
AUTO_CONFIRM="${2:-}"

if [[ -z "$VERSION" ]]; then
  echo "usage: $0 <x.y.z> [--yes]" >&2
  exit 1
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "version must be semver x.y.z" >&2
  exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "working tree must be clean before release" >&2
  exit 1
fi

TAG="v${VERSION}"
if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  echo "tag ${TAG} already exists locally" >&2
  exit 1
fi

CONFIRM_PHRASE="CONFIRM RELEASE ${TAG}"
if [[ "$AUTO_CONFIRM" != "--yes" ]]; then
  echo "About to release ${TAG}."
  echo "This will: bump versions, run tests/dry-run, create commit+tag, publish crates, push main+tag."
  read -r -p "Type '${CONFIRM_PHRASE}' to continue: " response
  if [[ "$response" != "$CONFIRM_PHRASE" ]]; then
    echo "release cancelled" >&2
    exit 1
  fi
fi

# 1) bump workspace version
perl -0777 -i -pe "s/(\\[workspace\\.package\\][^\\[]*?\\nversion\\s*=\\s*\")[^\"]+(\"\\n)/\${1}${VERSION}\${2}/s" Cargo.toml

# 2) sync explicit dependency versions for publishable crates
perl -i -pe 's/stardive-core = \{ version = "[^"]+", path = "\.\.\/stardive-core" \}/stardive-core = { version = "'"$VERSION"'", path = "..\/stardive-core" }/' \
  crates/stardive/Cargo.toml crates/stardive-api/Cargo.toml

# refresh lockfile and validate
cargo test --workspace --all-targets
cargo publish --dry-run --workspace --exclude stardive-api

git add Cargo.toml Cargo.lock crates/stardive/Cargo.toml crates/stardive-api/Cargo.toml
git commit -m "chore(release): ${TAG}"
git tag -a "${TAG}" -m "${TAG}"

# publish in dependency order
cargo publish -p stardive-core

published_stardive=0
for attempt in $(seq 1 20); do
  if cargo publish -p stardive; then
    published_stardive=1
    break
  fi
  echo "waiting for stardive-core index propagation (${attempt}/20) ..."
  sleep 15
done

if [[ "$published_stardive" -ne 1 ]]; then
  echo "failed to publish stardive after retries; commit/tag are local only" >&2
  echo "fix issue, then run: git push origin main && git push origin ${TAG}" >&2
  exit 1
fi

git push origin main
git push origin "${TAG}"

echo "release complete: ${TAG}"
