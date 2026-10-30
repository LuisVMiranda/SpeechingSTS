#!/usr/bin/env bash
set -eu

project_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
web_dir="$project_dir/web"
translation_file="$web_dir/launcher-i18n.tsv"
executable="$web_dir/target/release/speeching-web"
open_browser=1
build_release=1
auth_override=''
server_pid=''
command_args=()
has_command=0

select_language() {
  language=${SPEECHING_LAUNCHER_LANGUAGE:-}
  if [ -z "$language" ]; then
    while :; do
      printf 'Language / Idioma [EN/ES/PT] (EN): '
      language=''; IFS= read -r language || true
      case "$language" in ''|en|EN|es|ES|pt|PT) break ;; *) printf 'EN / ES / PT\n' ;; esac
    done
  fi
  case "$language" in
    ''|en|EN) language=en; language_column=2 ;;
    es|ES) language=es; language_column=3 ;;
    pt|PT) language=pt; language_column=4 ;;
    *) printf 'SPEECHING_LAUNCHER_LANGUAGE: EN / ES / PT\n' >&2; exit 1 ;;
  esac
  SPEECHING_LAUNCHER_LANGUAGE=$language; export SPEECHING_LAUNCHER_LANGUAGE
}

message() {
  key=$1; shift
  value=$(awk -F '\t' -v key="$key" -v col="$language_column" '$1 == key { print $col; found=1; exit } END { if (!found) exit 1 }' "$translation_file") || exit 1
  index=0
  for argument in "$@"; do
    token="{$index}"; value=${value//"$token"/"$argument"}; index=$((index + 1))
  done
  printf '%s' "$value"
}

fail() { printf 'Speeching web: %s\n' "$(message "$@")" >&2; exit 1; }
say() { printf '%s\n' "$(message "$@")"; }
usage() { fail usage_unix; }

set_auth_override() {
  case "$1" in builtin|off) auth_override=$1 ;; *) fail auth_argument ;; esac
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
    *) fail wrong_system ;;
  esac
  say detected_system "${distro:-Linux}" "$system"
}

ask_yes() {
  printf '%s %s ' "$(message "$1")" "$(message confirm_hint)"
  answer=''
  IFS= read -r answer || true
  case "$answer" in y|Y|yes|YES|s|S|si|SI|sí|SÍ|sim|SIM) return 0 ;; *) return 1 ;; esac
}

as_root() {
  if [ "$(id -u)" -eq 0 ]; then "$@"; return; fi
  command -v sudo >/dev/null 2>&1 || fail need_sudo
  sudo "$@"
}

install_build_dependencies() {
  case "$distro" in
    ubuntu|debian)
      ask_yes apt_ask || fail apt_missing
      as_root apt-get update
      as_root apt-get install -y build-essential pkg-config curl ca-certificates ;;
    fedora)
      ask_yes fedora_ask || fail fedora_missing
      as_root dnf install -y gcc gcc-c++ make pkgconf-pkg-config curl ca-certificates ;;
    macos)
      ask_yes xcode_ask || fail xcode_missing
      xcode-select --install || true
      fail xcode_finish ;;
    *) fail generic_missing ;;
  esac
}

install_rust() {
  ask_yes rust_ask || fail rust_missing
  installer=$(mktemp "${TMPDIR:-/tmp}/speeching-rustup.XXXXXX")
  if ! curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$installer"; then
    rm -f "$installer"
    fail rust_download_error
  fi
  if ! sh "$installer" -y --profile minimal --no-modify-path; then
    rm -f "$installer"
    fail rust_install_error
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
  case "$major:$minor" in *[!0-9:]*) fail rust_version_unknown ;; esac
  if [ "$major" -lt 1 ] || { [ "$major" -eq 1 ] && [ "$minor" -lt 88 ]; }; then
    fail rust_version_old
  fi
}

release_needs_build() {
  [ -x "$executable" ] || return 0
  # Test-only modules do not affect the release binary.
  [ -n "$(find "$web_dir/src" "$web_dir/static" "$web_dir/Cargo.toml" "$web_dir/Cargo.lock" "$translation_file" -type f ! -name 'tests.rs' ! -name '*_tests.rs' -newer "$executable" -print | head -n 1)" ]
}

prepare_release() {
  if [ "$build_release" -eq 0 ]; then
    [ -x "$executable" ] || fail no_build_unix
    return
  fi
  if release_needs_build; then
    ensure_toolchain
    say building
    CARGO_TARGET_DIR="$web_dir/target" cargo build --manifest-path "$web_dir/Cargo.toml" --release --locked
  fi
  [ -x "$executable" ] || fail no_executable
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
  [ "$count" -le 1 ] || fail db_multiple
  if [ "$count" -eq 1 ]; then printf '%s\n' "$found"; else printf '%s\n' "$default_data"; fi
}

prepare_key() {
  if [ -z "${SPEECHING_MASTER_KEY_FILE:-}" ] && [ -n "${SPEECHING_MASTER_KEY:-}" ]; then return; fi
  key=${SPEECHING_MASTER_KEY_FILE:-"${XDG_CONFIG_HOME:-$HOME/.config}/speeching-web/master.key"}
  SPEECHING_MASTER_KEY_FILE=$(resolve_web_path "$key"); export SPEECHING_MASTER_KEY_FILE
  [ -f "$SPEECHING_MASTER_KEY_FILE" ] && return
  if [ -e "$SPEECHING_WEB_DATA/speeching-web.sqlite3" ] || [ -e "$SPEECHING_WEB_DATA/speeching-web.sqlite3-wal" ]; then
    fail key_missing
  fi
  create_master_key
}

create_master_key() {
  key_dir=$(dirname "$SPEECHING_MASTER_KEY_FILE")
  mkdir -p "$key_dir"
  key_temp=$(mktemp "$key_dir/.speeching-key.XXXXXX")
  if ! key_value=$("$executable" keygen) || ! printf '%s' "$key_value" > "$key_temp" || ! chmod 600 "$key_temp"; then
    rm -f "$key_temp"; fail key_create_error
  fi
  unset key_value
  if ! ln "$key_temp" "$SPEECHING_MASTER_KEY_FILE" 2>/dev/null && [ ! -f "$SPEECHING_MASTER_KEY_FILE" ]; then
    rm -f "$key_temp"; fail key_install_error
  fi
  rm -f "$key_temp"
  say key_created
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
      *) fail origin_required ;;
    esac
  fi
}

prompt_authentication() {
  say auth_intro
  say auth_login
  say auth_guest
  printf '%s: ' "$(message auth_choice)"
  choice=''; IFS= read -r choice || true
  case "$choice" in ''|1) SPEECHING_AUTH=builtin ;; 2) SPEECHING_AUTH=off ;; *) fail auth_invalid ;; esac
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
  case "$SPEECHING_AUTH" in builtin|off) ;; *) fail auth_saved_invalid ;; esac
  export SPEECHING_AUTH
}

ensure_first_account() {
  [ "${SPEECHING_AUTH:-builtin}" = builtin ] || return 0
  [ "${SPEECHING_ALLOW_REGISTRATION:-true}" = false ] || return 0
  [ ! -f "$SPEECHING_WEB_DATA/speeching-web.sqlite3" ] || return 0
  say first_account_unix
  printf '%s: ' "$(message username)"
  username=''; IFS= read -r username || true
  [ -n "$username" ] || fail account_cancel_unix
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
  say starting_unix "${SPEECHING_ORIGIN:-http://127.0.0.1:8787}"
  "$executable" & server_pid=$!
  trap stop_server EXIT
  trap 'stop_server; exit 130' INT TERM
  [ "$open_browser" -eq 0 ] || open_tab
  if wait "$server_pid"; then result=0; else result=$?; fi
  server_pid=''
  return "$result"
}

select_language
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
