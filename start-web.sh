#!/usr/bin/env bash
set -eu

project_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
web_dir="$project_dir/web"
executable="$web_dir/target/release/speeching-web"
open_browser=1
build_release=1
auth_override=''
server_pid=''
command_args=()
has_command=0

fail() { printf 'Speeching web: %s\n' "$*" >&2; exit 1; }
say() { printf '%s\n' "$*"; }
usage() { fail 'Usage: start-web.sh [--no-open] [--no-build] [--auth builtin|off] [user add USERNAME]'; }

set_auth_override() {
  case "$1" in builtin|off) auth_override=$1 ;; *) fail '--auth needs builtin or off.' ;; esac
}

set_user_command() {
  if [ "$#" -ne 3 ] || [ "${2:-}" != add ]; then usage; fi
  command_args=("$@")
  has_command=1
}

parse_arguments() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --no-open) open_browser=0; shift ;;
      --no-build) build_release=0; shift ;;
      --auth)
        shift
        set_auth_override "${1:-}"
        shift ;;
      user)
        set_user_command "$@"
        break ;;
      *) usage ;;
    esac
  done
}

detect_system() {
  system=$(uname -s)
  distro=''
  case "$system" in
    Darwin) distro=macos ;;
    Linux)
      os_release=${SPEECHING_OS_RELEASE_FILE:-/etc/os-release}
      if [ -r "$os_release" ]; then
        distro=$(sed -n 's/^ID=//p' "$os_release" | head -n 1 | tr -d '"\047')
      fi ;;
    *) fail 'This launcher supports Linux and macOS. Use start-web.cmd on Windows.' ;;
  esac
  say "Detected ${distro:-Linux} ($system)."
}

ask_yes() {
  printf '%s [y/N] ' "$1"
  answer=''
  IFS= read -r answer || true
  case "$answer" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

as_root() {
  if [ "$(id -u)" -eq 0 ]; then "$@"; return; fi
  command -v sudo >/dev/null 2>&1 || fail 'Install sudo or run the displayed package commands as an administrator.'
  sudo "$@"
}

install_build_dependencies() {
  case "$distro" in
    ubuntu|debian)
      ask_yes 'Install build tools and curl with apt?' || fail 'Install build-essential, pkg-config, curl, and ca-certificates, then rerun.'
      as_root apt-get update
      as_root apt-get install -y build-essential pkg-config curl ca-certificates ;;
    fedora)
      ask_yes 'Install build tools and curl with dnf?' || fail 'Install gcc, gcc-c++, make, pkgconf-pkg-config, curl, and ca-certificates, then rerun.'
      as_root dnf install -y gcc gcc-c++ make pkgconf-pkg-config curl ca-certificates ;;
    macos)
      ask_yes 'Open the Xcode command line tools installer?' || fail 'Install Xcode command line tools, then rerun.'
      xcode-select --install || true
      fail 'Complete the command line tools installation, then rerun this launcher.' ;;
    *) fail 'Install a C compiler, make, and curl for this distribution, then rerun.' ;;
  esac
}

install_rust() {
  ask_yes 'Install the stable Rust toolchain with rustup?' || fail 'Install Rust 1.88 or newer, then rerun.'
  installer=$(mktemp "${TMPDIR:-/tmp}/speeching-rustup.XXXXXX")
  if ! curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$installer"; then
    rm -f "$installer"
    fail 'Could not download the Rust installer.'
  fi
  if ! sh "$installer" -y --profile minimal --no-modify-path; then
    rm -f "$installer"
    fail 'Rust installation did not complete.'
  fi
  rm -f "$installer"
  PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
  export PATH
}

ensure_toolchain() {
  missing=0
  for tool in cc make curl; do command -v "$tool" >/dev/null 2>&1 || missing=1; done
  if [ "$system" = Darwin ] && ! xcode-select -p >/dev/null 2>&1; then missing=1; fi
  [ "$missing" -eq 0 ] || install_build_dependencies
  if ! command -v cargo >/dev/null 2>&1 && [ -x "${CARGO_HOME:-$HOME/.cargo}/bin/cargo" ]; then
    PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"; export PATH
  fi
  if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then install_rust; fi
  validate_rust_version
}

validate_rust_version() {
  version=$(rustc --version | awk '{print $2}')
  major=${version%%.*}; rest=${version#*.}; minor=${rest%%.*}
  case "$major:$minor" in *[!0-9:]*) fail 'Could not determine the Rust version.' ;; esac
  if [ "$major" -lt 1 ] || { [ "$major" -eq 1 ] && [ "$minor" -lt 88 ]; }; then
    fail 'Rust 1.88 or newer is required. Update Rust, then rerun.'
  fi
}

release_needs_build() {
  [ -x "$executable" ] || return 0
  # Test-only modules do not affect the release binary.
  [ -n "$(find "$web_dir/src" "$web_dir/static" "$web_dir/Cargo.toml" "$web_dir/Cargo.lock" -type f ! -name 'tests.rs' ! -name '*_tests.rs' -newer "$executable" -print | head -n 1)" ]
}

prepare_release() {
  if [ "$build_release" -eq 0 ]; then
    [ -x "$executable" ] || fail 'The release executable is missing; rerun without --no-build.'
    return
  fi
  if release_needs_build; then
    ensure_toolchain
    say 'Building the current Rust web release...'
    CARGO_TARGET_DIR="$web_dir/target" cargo build --manifest-path "$web_dir/Cargo.toml" --release --locked
  fi
  [ -x "$executable" ] || fail 'The build did not produce the web executable.'
}

resolve_web_path() {
  case "$1" in /*) printf '%s\n' "$1" ;; *) printf '%s/%s\n' "$web_dir" "$1" ;; esac
}

default_data_directory() {
  if [ "$system" = Darwin ]; then default_data="$HOME/Library/Application Support/SpeechingWeb"
  else default_data="${XDG_DATA_HOME:-$HOME/.local/share}/speeching-web"; fi
  count=0
  for candidate in "$default_data" "$web_dir/speeching-web-data" "$project_dir/speeching-web-data"; do
    if [ -f "$candidate/speeching-web.sqlite3" ]; then found=$candidate; count=$((count + 1)); fi
  done
  [ "$count" -le 1 ] || fail 'Multiple web databases found. Set SPEECHING_WEB_DATA to choose one.'
  if [ "$count" -eq 1 ]; then printf '%s\n' "$found"; else printf '%s\n' "$default_data"; fi
}

prepare_key() {
  if [ -z "${SPEECHING_MASTER_KEY_FILE:-}" ] && [ -n "${SPEECHING_MASTER_KEY:-}" ]; then return; fi
  key=${SPEECHING_MASTER_KEY_FILE:-"${XDG_CONFIG_HOME:-$HOME/.config}/speeching-web/master.key"}
  SPEECHING_MASTER_KEY_FILE=$(resolve_web_path "$key"); export SPEECHING_MASTER_KEY_FILE
  [ -f "$SPEECHING_MASTER_KEY_FILE" ] && return
  if [ -e "$SPEECHING_WEB_DATA/speeching-web.sqlite3" ] || [ -e "$SPEECHING_WEB_DATA/speeching-web.sqlite3-wal" ]; then
    fail 'The existing database has no master key. Restore the original key before starting.'
  fi
  create_master_key
}

create_master_key() {
  key_dir=$(dirname "$SPEECHING_MASTER_KEY_FILE")
  mkdir -p "$key_dir"
  key_temp=$(mktemp "$key_dir/.speeching-key.XXXXXX")
  if ! key_value=$("$executable" keygen) || ! printf '%s' "$key_value" > "$key_temp" || ! chmod 600 "$key_temp"; then
    rm -f "$key_temp"; fail 'Could not create a protected master key.'
  fi
  unset key_value
  if ! ln "$key_temp" "$SPEECHING_MASTER_KEY_FILE" 2>/dev/null && [ ! -f "$SPEECHING_MASTER_KEY_FILE" ]; then
    rm -f "$key_temp"; fail 'Could not install the master key safely.'
  fi
  rm -f "$key_temp"
  say 'Web master key is ready. Back it up securely to recover saved provider keys.'
}

prepare_environment() {
  umask 077
  data=${SPEECHING_WEB_DATA:-$(default_data_directory)}
  SPEECHING_WEB_DATA=$(resolve_web_path "$data"); export SPEECHING_WEB_DATA
  mkdir -p "$SPEECHING_WEB_DATA"
  prepare_key
  if [ -z "${SPEECHING_ORIGIN:-}" ] && [ -n "${SPEECHING_LISTEN:-}" ]; then
    case "$SPEECHING_LISTEN" in
      127.0.0.1:*|'[::1]':*) SPEECHING_ORIGIN="http://$SPEECHING_LISTEN"; export SPEECHING_ORIGIN ;;
      *) fail 'Set SPEECHING_ORIGIN to the browser origin for this listen address.' ;;
    esac
  fi
}

prompt_authentication() {
  say 'Choose how people access Speeching web:'
  say '  1) Built-in login and separate accounts (recommended)'
  say '  2) No login (one shared guest; protect access externally)'
  printf 'Choice [1]: '
  choice=''; IFS= read -r choice || true
  case "$choice" in ''|1) SPEECHING_AUTH=builtin ;; 2) SPEECHING_AUTH=off ;; *) fail 'Choose 1 or 2 and restart.' ;; esac
  printf '%s\n' "$SPEECHING_AUTH" > "$auth_file"
}

load_authentication() {
  if [ -f "$auth_file" ]; then SPEECHING_AUTH=$(cat "$auth_file"); else prompt_authentication; fi
}

select_authentication() {
  auth_file="$SPEECHING_WEB_DATA/launcher-auth"
  if [ -n "$auth_override" ]; then
    SPEECHING_AUTH=$auth_override
    printf '%s\n' "$SPEECHING_AUTH" > "$auth_file"
  elif [ -n "${SPEECHING_AUTH:-}" ]; then return
  else load_authentication
  fi
  case "$SPEECHING_AUTH" in builtin|off) ;; *) fail 'The saved launcher authentication setting is invalid.' ;; esac
  export SPEECHING_AUTH
}

ensure_first_account() {
  [ "${SPEECHING_AUTH:-builtin}" = builtin ] || return 0
  [ "${SPEECHING_ALLOW_REGISTRATION:-true}" = false ] || return 0
  [ ! -f "$SPEECHING_WEB_DATA/speeching-web.sqlite3" ] || return 0
  say 'Registration is disabled. Create the first web account.'
  printf 'Username: '
  username=''; IFS= read -r username || true
  [ -n "$username" ] || fail 'Account setup canceled. Run start-web.sh user add USERNAME to create an account.'
  "$executable" user add "$username"
}

open_tab() {
  url=${SPEECHING_ORIGIN:-http://127.0.0.1:8787}
  if [ "$system" = Linux ] && [ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then return; fi
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
    kill -0 "$server_pid" 2>/dev/null || return
    if command -v curl >/dev/null 2>&1 && curl -fsS --max-time 1 "$url/api/mode" >/dev/null 2>&1; then
      if [ "$system" = Darwin ]; then browser=open; else browser=xdg-open; fi
      if command -v "$browser" >/dev/null 2>&1; then
        (unset SPEECHING_MASTER_KEY SPEECHING_PROXY_SECRET; "$browser" "$url/" >/dev/null 2>&1) &
      fi
      return
    fi
    sleep 1
  done
}

stop_server() {
  [ -n "$server_pid" ] || return 0
  kill -INT "$server_pid" 2>/dev/null || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    kill -0 "$server_pid" 2>/dev/null || break
    sleep .1
  done
  kill "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
  server_pid=''
}

start_service() {
  say "Starting Speeching web at ${SPEECHING_ORIGIN:-http://127.0.0.1:8787}. Press Ctrl+C to stop."
  "$executable" & server_pid=$!
  trap stop_server EXIT
  trap 'stop_server; exit 130' INT TERM
  [ "$open_browser" -eq 0 ] || open_tab
  if wait "$server_pid"; then result=0; else result=$?; fi
  server_pid=''
  return "$result"
}

parse_arguments "$@"
detect_system
prepare_release
prepare_environment
if [ "$has_command" -eq 1 ]; then
  "$executable" "${command_args[@]}"
else
  select_authentication
  ensure_first_account
  start_service
fi
