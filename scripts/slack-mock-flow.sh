#!/usr/bin/env bash
# End-to-end Slack mock flow — exercises login, look, say, tell, movement, OOC.
# No Slack credentials or network required.
set -euo pipefail

cd "$(dirname "$0")/.."

export SLACK_MOCK=1
export DATABASE_URL="${DATABASE_URL:-sqlite::memory:}"
export DEFAULT_PLAYER="${DEFAULT_PLAYER:-player:admin-001}"
export SLACK_WORLD_CHANNEL="${SLACK_WORLD_CHANNEL:-C_WORLD}"
export RUST_LOG="${RUST_LOG:-warn}"

echo "==> Building slack binary"
cargo build --bin slack --quiet

echo "==> Running mock group-play session"
printf '%s\n' \
  'U_ALICE D_ALICE login player:hero-001' \
  'U_BOB D_BOB login player:hero-002' \
  'U_ALICE D_ALICE look' \
  'U_ALICE D_ALICE say hello void' \
  'U_ALICE D_ALICE tell Bob secret' \
  'U_ALICE D_ALICE go north' \
  'U_ALICE C_WORLD brb dinner' \
  'U_ALICE D_ALICE quit' \
  | cargo run --bin slack --quiet 2>&1

echo "==> Running M6 test suite"
make test-m6

echo "OK — Slack mock flow and M6 tests passed"