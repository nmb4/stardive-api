#!/bin/sh
set -eu

obscura_pid=""
stardive_pid=""

stop_children() {
  if [ -n "$stardive_pid" ]; then
    kill "$stardive_pid" 2>/dev/null || true
  fi
  if [ -n "$obscura_pid" ]; then
    kill "$obscura_pid" 2>/dev/null || true
  fi
  if [ -n "$stardive_pid" ]; then
    wait "$stardive_pid" 2>/dev/null || true
  fi
  if [ -n "$obscura_pid" ]; then
    wait "$obscura_pid" 2>/dev/null || true
  fi
}

trap stop_children EXIT INT TERM

case "${STARDIVE_ENABLE_OBSCURA:-true}" in
  1|true|TRUE|yes|YES|on|ON)
    obscura_port="${STARDIVE_OBSCURA_MCP_PORT:-8081}"
    obscura mcp --http --port "$obscura_port" &
    obscura_pid=$!
    attempts=0
    until curl -s -o /dev/null "http://127.0.0.1:${obscura_port}/mcp"; do
      if ! kill -0 "$obscura_pid" 2>/dev/null; then
        echo "obscura HTTP MCP server exited during startup" >&2
        exit 1
      fi
      attempts=$((attempts + 1))
      if [ "$attempts" -ge 50 ]; then
        echo "timed out waiting for obscura HTTP MCP server" >&2
        exit 1
      fi
      sleep 0.1
    done
    ;;
esac

stardive-api &
stardive_pid=$!
wait "$stardive_pid"
