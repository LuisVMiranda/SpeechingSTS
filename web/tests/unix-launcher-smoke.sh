#!/usr/bin/env bash
set -euE

project=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)
temp_parent=${TMPDIR:-/tmp}
temp=$(mktemp -d "$temp_parent/speeching-launcher.XXXXXX")
launcher_pid=''
mkdir -p "$temp/home"
touch "$temp/.speeching-test"

cleanup() {
  if [ -n "$launcher_pid" ]; then kill "$launcher_pid" 2>/dev/null || true; wait "$launcher_pid" 2>/dev/null || true; fi
  case "$temp" in "$temp_parent"/speeching-launcher.*) ;; *) echo 'Refusing to remove an unexpected path.' >&2; exit 1 ;; esac
  [ -f "$temp/.speeching-test" ] || { echo 'Test directory marker is missing.' >&2; exit 1; }
  rm -rf -- "$temp"
}
trap cleanup EXIT
trap 'echo "Unix launcher check failed near line $LINENO" >&2; [ ! -f "$temp/log" ] || cat "$temp/log" >&2' ERR

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()'
}

wait_ready() {
  for _ in $(seq 1 100); do
    if curl -fsS "http://127.0.0.1:$port/api/mode" > "$temp/mode" 2>/dev/null; then return; fi
    kill -0 "$launcher_pid" 2>/dev/null || { cat "$temp/log"; return 1; }
    sleep .1
  done
  cat "$temp/log"
  return 1
}

stop_launcher() {
  kill "$launcher_pid" 2>/dev/null || true
  wait "$launcher_pid" 2>/dev/null || true
  launcher_pid=''
}

start_launcher() {
  port=$(free_port)
  printf '%s\n' "$1" | env -i PATH="$PATH" HOME="$temp/home" \
    SPEECHING_WEB_DATA="$temp/private data" SPEECHING_MASTER_KEY_FILE="$temp/private config/master.key" \
    SPEECHING_LISTEN="127.0.0.1:$port" \
    bash "$project/start-web.sh" --no-build --no-open > "$temp/log" 2>&1 &
  launcher_pid=$!
  wait_ready
}

test_persistence() {
  start_launcher 2
  grep -q '"auth":"off"' "$temp/mode"
  [ "$(cat "$temp/private data/launcher-auth")" = off ]
  key="$temp/private config/master.key"
  [ "$(wc -c < "$key" | tr -d ' ')" = 43 ]
  if [ "$(uname -s)" = Darwin ]; then permissions=$(stat -f '%Lp' "$key"); else permissions=$(stat -c '%a' "$key"); fi
  [ "$permissions" = 600 ]
  checksum=$(cksum "$key")
  origin="http://127.0.0.1:$port"
  curl -fsS -X POST -H "Origin: $origin" -H 'Content-Type: application/json' \
    -d '{"key":"synthetic-provider-key"}' "$origin/api/keys/gemini" >/dev/null
  stop_launcher
  start_launcher ''
  grep -q '"auth":"off"' "$temp/mode"
  if grep -q 'Choice \[1\]' "$temp/log"; then echo 'The saved authentication choice was not reused.' >&2; return 1; fi
  [ "$(cksum "$key")" = "$checksum" ]
  curl -fsS "http://127.0.0.1:$port/api/me" | grep -q '"gemini":true'
  stop_launcher
}

test_missing_key() {
  mkdir -p "$temp/existing"
  printf 'existing data' > "$temp/existing/speeching-web.sqlite3"
  before=$(cksum "$temp/existing/speeching-web.sqlite3")
  if env -i PATH="$PATH" HOME="$temp/home" SPEECHING_WEB_DATA="$temp/existing" \
    SPEECHING_MASTER_KEY_FILE="$temp/missing/master.key" SPEECHING_AUTH=off \
    bash "$project/start-web.sh" --no-build --no-open > "$temp/missing.log" 2>&1; then
    echo 'Missing master key should have prevented startup.' >&2; exit 1
  fi
  grep -q 'no master key' "$temp/missing.log"
  [ ! -e "$temp/missing/master.key" ]
  [ "$(cksum "$temp/existing/speeching-web.sqlite3")" = "$before" ]
}

test_auth_override() {
  port=$(free_port)
  env -i PATH="$PATH" HOME="$temp/home" SPEECHING_WEB_DATA="$temp/private data" \
    SPEECHING_MASTER_KEY_FILE="$temp/private config/master.key" SPEECHING_LISTEN="127.0.0.1:$port" \
    bash "$project/start-web.sh" --no-build --no-open --auth builtin > "$temp/log" 2>&1 &
  launcher_pid=$!
  wait_ready
  grep -q '"auth":"builtin"' "$temp/mode"
  [ "$(cat "$temp/private data/launcher-auth")" = builtin ]
  stop_launcher
}

test_distro_commands() {
  [ "$(uname -s)" = Linux ] || return 0
  fake="$temp/fake-bin"
  mkdir -p "$fake"
  for tool in uname dirname sed head tr mkdir cat chmod awk find; do ln -s "$(command -v "$tool")" "$fake/$tool"; done
  cat > "$fake/id" <<'SCRIPT'
#!/bin/sh
printf '0\n'
SCRIPT
  cat > "$fake/rustc" <<'SCRIPT'
#!/bin/sh
printf 'rustc 1.99.0\n'
SCRIPT
  cat > "$fake/cargo" <<'SCRIPT'
#!/bin/sh
printf 'cargo:%s\n' "$*" >> "$SPEECHING_TEST_TRACE"
mkdir -p "$CARGO_TARGET_DIR/release"
cat > "$CARGO_TARGET_DIR/release/speeching-web" <<'SERVER'
#!/bin/sh
printf 'server:%s\n' "$*" >> "$SPEECHING_TEST_TRACE"
SERVER
chmod +x "$CARGO_TARGET_DIR/release/speeching-web"
SCRIPT
  for tool in apt-get dnf; do
    cat > "$fake/$tool" <<'SCRIPT'
#!/bin/sh
printf '%s:%s\n' "${0##*/}" "$*" >> "$SPEECHING_TEST_TRACE"
SCRIPT
  done
  chmod +x "$fake/id" "$fake/rustc" "$fake/cargo" "$fake/apt-get" "$fake/dnf"
  for distro in ubuntu debian fedora; do
    fixture="$temp/fixture-$distro"
    mkdir -p "$fixture/web/src" "$fixture/web/static" "$fixture/data"
    : > "$fixture/web/Cargo.toml"
    : > "$fixture/web/Cargo.lock"
    cp "$project/start-web.sh" "$fixture/start-web.sh"
    printf 'ID="%s"\n' "$distro" > "$fixture/os-release"
    trace="$fixture/commands"
    printf 'y\n' | env -i PATH="$fake" HOME="$temp/home" SPEECHING_OS_RELEASE_FILE="$fixture/os-release" \
      SPEECHING_WEB_DATA="$fixture/data" SPEECHING_MASTER_KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA \
      SPEECHING_TEST_TRACE="$trace" "$BASH" "$fixture/start-web.sh" user add demo > "$fixture/log" 2>&1 || { cat "$fixture/log"; return 1; }
    if [ "$distro" = fedora ]; then
      grep -q '^dnf:install -y gcc' "$trace"
    else
      grep -q '^apt-get:update' "$trace"
      grep -q '^apt-get:install -y build-essential' "$trace"
    fi
    grep -q '^server:user add demo' "$trace"
    test_source="$fixture/web/src/only_tests.rs"
    printf '// test-only fixture\n' > "$test_source"
    python3 - "$test_source" <<'PY'
import os, sys, time
future = time.time() + 5
os.utime(sys.argv[1], (future, future))
PY
    builds=$(grep -c '^cargo:' "$trace")
    env -i PATH="$fake" HOME="$temp/home" SPEECHING_OS_RELEASE_FILE="$fixture/os-release" \
      SPEECHING_WEB_DATA="$fixture/data" SPEECHING_MASTER_KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA \
      SPEECHING_TEST_TRACE="$trace" "$BASH" "$fixture/start-web.sh" user add demo > "$fixture/log" 2>&1 || { cat "$fixture/log"; return 1; }
    [ "$(grep -c '^cargo:' "$trace")" = "$builds" ]
  done
}

test_persistence
test_missing_key
test_auth_override
test_distro_commands
echo 'Unix launcher checks passed: choice, permissions, key persistence, recovery guard, authentication override, and package commands.'
