#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

SSH_HOST="stardive"
REMOTE_REPO="/root/repos/stardive-api"
BRANCH="main"
APP_HOST="api.stardive.space"
IMAGE="localhost:5000/stardive-api:once-local"
PUBLIC_BASE_URL="https://api.stardive.space"
PUSH_LOCAL=1

usage() {
  cat <<'USAGE'
Automate stardive-api server rollout via SSH + once.

Usage:
  ./scripts/deploy-stardive-server.sh [options]

Options:
  --ssh-host <host>         SSH host alias (default: stardive)
  --remote-repo <path>      Remote repo path (default: /root/repos/stardive-api)
  --branch <name>           Branch to deploy (default: main)
  --app-host <host>         once app host (default: api.stardive.space)
  --image <ref>             Image ref for build/push/update
                            (default: localhost:5000/stardive-api:once-local)
  --public-base-url <url>   URL for final health checks
                            (default: https://api.stardive.space)
  --no-push                 Skip local git push before deploy
  -h, --help                Show help

Examples:
  ./scripts/deploy-stardive-server.sh
  ./scripts/deploy-stardive-server.sh --branch main --app-host api.stardive.space
  ./scripts/deploy-stardive-server.sh --no-push
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --ssh-host)
      SSH_HOST="${2:-}"
      shift 2
      ;;
    --remote-repo)
      REMOTE_REPO="${2:-}"
      shift 2
      ;;
    --branch)
      BRANCH="${2:-}"
      shift 2
      ;;
    --app-host)
      APP_HOST="${2:-}"
      shift 2
      ;;
    --image)
      IMAGE="${2:-}"
      shift 2
      ;;
    --public-base-url)
      PUBLIC_BASE_URL="${2:-}"
      shift 2
      ;;
    --no-push)
      PUSH_LOCAL=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

required_cmds=(git ssh)
for cmd in "${required_cmds[@]}"; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing required command: $cmd" >&2
    exit 1
  fi
done

if [[ "$PUSH_LOCAL" -eq 1 ]]; then
  if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "working tree must be clean before pushing/deploying; commit or stash first" >&2
    exit 1
  fi

  echo "==> pushing local HEAD to origin/${BRANCH}"
  git push origin "HEAD:${BRANCH}"
else
  echo "==> skipping local push (--no-push)"
fi

echo "==> deploying on ${SSH_HOST}"
ssh "$SSH_HOST" "set -euo pipefail
cd '${REMOTE_REPO}'
git fetch origin
git checkout '${BRANCH}'
git pull --ff-only origin '${BRANCH}'
docker build -t '${IMAGE}' .
docker push '${IMAGE}'
once update '${APP_HOST}' --image '${IMAGE}'
"

echo "==> waiting for healthy container"
ssh "$SSH_HOST" "set -euo pipefail
for _ in \$(seq 1 30); do
  container_state=\$(docker ps --format '{{.Names}} {{.Status}}' | grep '^once-app-stardive-api' || true)
  if [[ \"\$container_state\" == *'(healthy)'* ]]; then
    echo \"\$container_state\"
    exit 0
  fi
  sleep 2
done
echo 'timed out waiting for healthy once-app-stardive-api container' >&2
exit 1
"

echo "==> verifying public endpoints"
curl -fsS "${PUBLIC_BASE_URL}/up" >/dev/null
curl -fsS "${PUBLIC_BASE_URL}/v1/lostandfound/health" >/dev/null

echo "deploy complete"
echo "  app:   ${APP_HOST}"
echo "  image: ${IMAGE}"
