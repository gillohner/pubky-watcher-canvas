#!/usr/bin/env bash
set -euo pipefail

cargo run &
server_pid=$!
trap 'kill "$server_pid" 2>/dev/null || true' EXIT INT TERM

cd web
npm run dev
