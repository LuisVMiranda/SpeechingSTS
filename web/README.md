# Speeching web

Speeching web is a separate Rust service for spoken language practice in a browser. It serves its own HTML, CSS, and JavaScript and relays audio to Gemini Live or OpenAI Realtime. Node.js and WebAssembly are not required to run it. It does not read, modify, or share the Windows app's settings, credential store, or conversation files.

The interface provides a microphone-first practice view, voice and pace choices, optional practice preferences, voice activation or push to talk with a reassignable shortcut, a noise gate, live transcripts, saved conversations, search and sorting, rename/delete, copy/download, optional opening of the log after a session, light/dark themes, and English, Spanish, and Brazilian Portuguese interface text. The keyboard shortcut works while the browser tab is active; browser or operating-system shortcuts may take priority. The web version has its own settings and a smaller, explicitly supported set of practice languages than the desktop app.

## Build

Install Rust 1.88 or newer and a C compiler supported by the Rust toolchain (the SQLite library is compiled into the executable). From this directory:

```sh
cargo build --locked --release
```

The executable is `target/release/speeching-web` on Linux and macOS and `target\release\speeching-web.exe` on Windows. The repository's web CI is configured to build and test all three operating systems. The app's static assets are embedded in the executable.

### Guided launchers

On Windows, double-click the root-folder `start-web.cmd`. On Linux or macOS, run `bash start-web.sh` from the root folder. Choose English (`EN`), Spanish (`ES`), or Brazilian Portuguese (`PT`) first to use that language for the setup prompts. For unattended launches, set `SPEECHING_LAUNCHER_LANGUAGE` to `en`, `es`, or `pt` to skip the question. Each launcher builds the current Rust release when needed and opens a normal tab in the default browser; on a headless server it prints the address instead. Keep the launcher running while using the app and press Ctrl+C to stop it.

The launcher asks whether to use built-in login or a shared guest and remembers the choice in the web data directory. Built-in accounts can register on the login page; if registration is disabled, the launcher prompts for the first account. The login page's **Forgot password?** action directs users to their server administrator. After verifying the user's identity, an administrator can reset that account with `start-web.cmd user add USERNAME` on Windows or `./start-web.sh user add USERNAME` on Linux/macOS. The account's conversations remain available. Recovery does not send email or reset a password automatically.

When a build is needed, the launcher checks the C toolchain, `curl`, `pkg-config`, and a system CA certificate store. On Ubuntu, Debian, and Fedora it offers to install missing build dependencies with `apt` or `dnf`; on macOS it offers the Xcode command line tools installer and, if Homebrew is available, can install `pkg-config` through it. macOS root certificates are checked in the system keychain; a missing store requires system repair. It offers the standard Rust installation if Rust is missing. These installations run only if you agree, may require administrator access and network access, and Rust must be version 1.88 or newer. Other Linux distributions need these dependencies installed separately.

The launchers create a master key with restricted permissions on first setup and refuse to replace a missing key when a database already exists. Back up that key securely. Provider API keys are entered later in web settings and remain saved encrypted.

On Windows, the default master key is `%USERPROFILE%\.config\speeching-web\master.key` and the data directory is `%LOCALAPPDATA%\SpeechingWeb`. On Linux, the defaults are `~/.config/speeching-web/master.key` and `~/.local/share/speeching-web` (or the corresponding XDG directories); on macOS, the data directory defaults to `~/Library/Application Support/SpeechingWeb`. If a launcher finds one existing web database at an old default location in the project or `web` folder, it uses that database instead. If multiple databases exist, set `SPEECHING_WEB_DATA` to choose one. The launchers respect `SPEECHING_MASTER_KEY_FILE`, `SPEECHING_MASTER_KEY`, and other service environment settings and derive the local browser origin from a custom loopback listen port when needed.

To change the remembered authentication choice, run `start-web.cmd -AuthMode builtin` or `start-web.cmd -AuthMode off` on Windows, or `bash start-web.sh --auth builtin` / `bash start-web.sh --auth off` on Linux/macOS. An explicitly supplied `SPEECHING_AUTH` setting takes precedence over the remembered choice unless a launcher option is supplied. Choosing the guest mode uses a separate shared identity; it does not delete built-in account histories. Use `-NoOpen` / `--no-open` to leave the browser closed, or `-NoBuild` / `--no-build` to skip the source freshness check. On Windows, `SPEECHING_WEB_BROWSER` can override the system default browser. The original Node prototype remains available through `legacy-web\start.cmd`.

## Set up a local account

Each installation needs one persistent 32-byte master key. Generate it once, store it separately from the database, restrict who can read it, and back it up securely. Losing it makes saved provider keys unrecoverable. Changing it does not automatically re-encrypt existing keys.

Example for Linux/macOS, from `web/`:

```sh
umask 077
mkdir -p "$HOME/.config/speeching-web" "$HOME/.local/share/speeching-web"
./target/release/speeching-web keygen > "$HOME/.config/speeching-web/master.key"
chmod 600 "$HOME/.config/speeching-web/master.key"
export SPEECHING_MASTER_KEY_FILE="$HOME/.config/speeching-web/master.key"
export SPEECHING_WEB_DATA="$HOME/.local/share/speeching-web"
./target/release/speeching-web user add alice
./target/release/speeching-web
```

Example for PowerShell, from `web/`:

```powershell
$speechingConfig = Join-Path $env:USERPROFILE '.config\speeching-web'
New-Item -ItemType Directory -Force -Path $speechingConfig | Out-Null
& .\target\release\speeching-web.exe keygen | Set-Content -Encoding ascii -NoNewline (Join-Path $speechingConfig 'master.key')
$env:SPEECHING_MASTER_KEY_FILE = Join-Path $speechingConfig 'master.key'
$env:SPEECHING_WEB_DATA = Join-Path $env:LOCALAPPDATA 'SpeechingWeb'
& .\target\release\speeching-web.exe user add alice
& .\target\release\speeching-web.exe
```

On Windows, restrict the master key and data directory with NTFS permissions to the service account and administrators. The account command prompts for the password without echoing it; use at least 12 bytes. Running `user add` with an existing username resets its password and invalidates that user's existing built-in sessions. Add one account per person. Usernames are case-insensitive in built-in mode and may contain letters, digits, `.`, `_`, `-`, and `@`.

Open `http://127.0.0.1:8787/`, sign in, then add a provider API key in settings. A saved key is never returned to the browser. The server needs the key in memory while it connects to the provider. Provider usage, model availability, rate limits, and any costs are determined by the provider and account; this project does not promise a free or unlimited tier.

For a local single-user experiment, set `SPEECHING_AUTH=off` before starting. The service then uses one shared guest identity. It refuses a non-loopback listen address in this mode, but a reverse proxy could still expose it: protect the proxy and network yourself. Use built-in or proxy mode for separate users.

## Deploy behind HTTPS

The service itself speaks HTTP and binds to `127.0.0.1:8787` by default. For a VPS, keep it on loopback or a private network and put a TLS-terminating reverse proxy in front of it. Browsers require HTTPS for microphone access except on localhost. Configure `SPEECHING_ORIGIN` to the exact public origin, such as `https://example.com`; omit any path and include a nonstandard port if used. The proxy must pass WebSocket upgrades.

The service can be mounted at the root of a dedicated host or under a path prefix if the proxy strips that prefix. For example, a Caddy site can use the following route within its existing configuration:

```caddyfile
redir /speeching /speeching/ 308
handle_path /speeching/* {
    reverse_proxy 127.0.0.1:8787
}
```

Use `https://example.com/speeching/`, including the trailing slash. This example only shows routing and TLS termination; configure authentication separately. The UI is a standalone page and deliberately refuses iframe embedding.

### Authentication modes

Set `SPEECHING_AUTH` before starting the server:

- `builtin` (default): self-registered or administrator-created accounts have separate histories and provider keys. Set `SPEECHING_ALLOW_REGISTRATION=false` to restrict account creation to the administrator's command. Passwords are Argon2id hashes. Cookies are HttpOnly, SameSite=Strict, and Secure on HTTPS; they expire after 12 hours. After three failed sign-in attempts, both the account and source IP enter a cooldown (five minutes by default). Registration also has a per-IP and per-account limit of three attempts during that period. Failed attempts and cooldowns are logged without passwords, keys, audio, or transcript text. Signing out or resetting a password revokes the user's built-in session, including active sockets within the relay's polling and write timeout.
- `proxy`: the upstream authentication gateway must authenticate **every** HTTP request and WebSocket upgrade, remove any client-supplied `X-Speeching-User` header, and add that header using only its verified user identity. It must also strip any incoming `X-Speeching-Proxy-Secret` and add the shared proxy secret described below. Set `SPEECHING_TRUSTED_PROXY` to the exact connecting proxy IP; both the peer IP and secret must match. Identifiers must use the same 1–64-character alphabet as built-in usernames and are case-sensitive. Distinct identifiers get distinct histories and credentials. The gateway must close existing WebSockets if it needs immediate revocation of a proxy-authenticated session.
- `off`: one shared guest identity, intended only for explicitly protected single-user deployments. It does not provide per-user isolation or a login cooldown.

Generate a separate proxy secret with `speeching-web keygen`, keep it out of source control and logs, and configure it on both the Rust service and gateway. Set `SPEECHING_PROXY_SECRET_FILE` to a protected file on the service, or use `SPEECHING_PROXY_SECRET` through a secret manager. Configure the gateway to overwrite `X-Speeching-Proxy-Secret` with that same value and to overwrite `X-Speeching-User` with the authenticated identity. The secret must be 32–256 printable non-space ASCII characters. Do not reuse the master key. The shared secret prevents other local processes from simply forging identity headers; anyone who can read it can impersonate proxy users. Never enable proxy mode unless the upstream proxy strips and replaces both headers. Do not expose the Rust listener directly around the proxy. Built-in and proxy account namespaces are intentionally separate, even when their names match.

The cooldown uses the direct peer IP by default. Behind a reverse proxy, this means failures from different users can share one proxy-IP cooldown. To use individual client IPs, configure the trusted proxy to remove any incoming `X-Speeching-Client-IP` and set it to one verified IP address, then set `SPEECHING_TRUST_CLIENT_IP_HEADER=true` and `SPEECHING_TRUSTED_PROXY` to that proxy's exact address. The gateway must also overwrite the shared `X-Speeching-Proxy-Secret` header on these requests, and the service must have the same secret configured. Do not trust a client-provided header. Cooldowns and built-in sessions are in memory: they reset on a service restart and are not shared between multiple service processes. Use one process or enforce distributed rate limits at the gateway.

## Configuration

| Environment variable | Default | Purpose |
| --- | --- | --- |
| `SPEECHING_LISTEN` | `127.0.0.1:8787` | HTTP listener address. |
| `SPEECHING_ORIGIN` | `http://127.0.0.1:8787` | Exact browser origin for POST, PATCH, DELETE, and WebSocket checks. Remote origins must use HTTPS. |
| `SPEECHING_AUTH` | `builtin` | `builtin`, `proxy`, or `off`. |
| `SPEECHING_ALLOW_REGISTRATION` | `true` | Set to `false` to disable public self-registration in built-in mode. |
| `SPEECHING_TRUSTED_PROXY` | `127.0.0.1` | Exact trusted peer IP for proxy identity and the optional client-IP header. |
| `SPEECHING_PROXY_SECRET_FILE` | none | Protected file containing the shared proxy secret; required in proxy mode or when trusting the client-IP header. |
| `SPEECHING_PROXY_SECRET` | none | Alternative shared proxy secret; the file setting takes precedence. |
| `SPEECHING_TRUST_CLIENT_IP_HEADER` | `false` | Trust `X-Speeching-Client-IP` only from the configured proxy. |
| `SPEECHING_WEB_DATA` | `./speeching-web-data` | Separate SQLite data directory, relative to the process working directory. Unix permissions are restricted to `0700` on startup. |
| `SPEECHING_MASTER_KEY_FILE` | none | File containing the unpadded base64url master key. |
| `SPEECHING_MASTER_KEY` | none | Alternative key value; the file setting takes precedence. Prefer a protected file or secret manager over an environment value. |
| `SPEECHING_MAX_SESSIONS` | `8` | Maximum concurrent live relays, clamped to 1–64. |
| `SPEECHING_COOLDOWN_SECONDS` | `300` | Failed-login cooldown, clamped to 30–86,400 seconds. |
| `SPEECHING_GEMINI_URL` | Google Live WebSocket endpoint | Admin-controlled Gemini-compatible endpoint. |
| `SPEECHING_GEMINI_MODEL` | `gemini-3.8-live` | Gemini Live model ID; choose one available to your account. |
| `SPEECHING_OPENAI_URL` | `wss://api.openai.com/v1/realtime` | Admin-controlled OpenAI-compatible endpoint. |
| `SPEECHING_OPENAI_MODEL` | `gpt-realtime-2.1` | OpenAI Realtime model ID; choose one available to your account. |

Remote provider endpoints must use `wss://`; local test endpoints may use `ws://` on loopback. Endpoints cannot contain credentials, query parameters, or fragments. Custom providers must speak the selected Gemini or OpenAI protocol; an arbitrary URL does not automatically make another protocol compatible. Settings and the model apply to the next session.

## Privacy, safety, and limits

Saved provider keys use AES-256-GCM with a fresh nonce and are bound to the account and provider. This protects the database's stored key values; it does not protect against someone who can read the master key, control the service process, or administer the server. Passwords are hashed, not encrypted. Conversation text and timestamps are stored in plaintext in the private web database and sent to the selected provider as needed. Back up the database and master key separately, and use storage encryption if transcripts need encryption at rest. Browser preferences are stored locally for each browser/account.

The server validates the provider, voice, language, pace, and mode against allowlists before constructing the system instruction. Saved conversation content and optional practice preferences are passed as lower-priority dialogue, and model output is rendered as text. The model receives no server tools, shell, filesystem access, provider credentials, or browsing capabilities. These boundaries reduce the impact of prompt injection; they cannot guarantee that a model will ignore malicious instructions spoken or included in a conversation.

Conversations are checkpointed at completed turns and when the connection ends; a process crash may lose the current unfinished turn. A session stops at one hour or its transcript limit. Providers may end a connection earlier; Gemini Live connections commonly have a roughly ten-minute connection lifetime. If the connection ends, select the saved conversation and start it again to continue with recent transcript context. Automatic provider session resumption is not implemented. Context sent on continuation is bounded to the most recent 80 messages and 24,000 characters, and can omit older material. History remains browsable in pages; copy and download retrieve all pages.

## Validation

```sh
cargo fmt -- --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

The test suite uses loopback mock providers and synthetic credentials; it does not need a real API key. A real provider conversation still requires a browser with microphone permission, a valid provider key, and an available compatible Live model.

On Windows, `powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File tests/launcher-smoke.ps1` checks the launcher with isolated data. On Linux or macOS, run `bash tests/unix-launcher-smoke.sh`. Build the web release first. The tests cover authentication choice, protected key persistence, and missing-key handling; the Windows checks also cover browser launch, port conflicts, and release builds, and the Linux checks simulate the distribution package commands.

One Windows x86-64 release measurement was approximately 8.77 MiB working set and 2.18 MiB private memory while idle; the executable was approximately 6.05 MiB. This excludes the browser, reverse proxy, active calls, and operating-system cache. Memory use varies with platform, concurrent users, and workload.

An optional end-to-end browser smoke test additionally needs Node.js 24 or newer and Chromium/Chrome; these are development tools, not production dependencies. Build the release executable first, then run:

```sh
node tests/browser-smoke.mjs
```

Set `CHROME` to the browser executable if it is not in the default location. The test creates an isolated temporary database, uses a fake microphone and local mock provider, checks the subpath reverse proxy and push-to-talk controls, and removes its temporary files afterward.
