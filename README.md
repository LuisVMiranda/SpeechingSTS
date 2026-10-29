# Speeching

Speeching is a small native Windows app for spoken language practice. The Rust app captures your microphone, streams audio to a live model, and plays the reply. It does not run inside a browser or require a local web server.

## Start

1. Double-click `speeching.exe` (or `start.vbs`). The executable opens without a terminal.
2. Open **Settings** using the sliders icon. In **Provider**, choose a provider and paste its API key. In **Practice**, choose your language, level and voice, then save.
3. Press the large microphone to start or end a conversation. While connected, the small microphone in the top bar mutes or unmutes you. Headphones are recommended; the native app does not provide acoustic echo cancellation.

The app saves each provider's API key in Windows Credential Manager for the current Windows user, separate from the project and settings file. A pasted key is saved automatically; typed keys are saved when you leave the field, choose **Save key**, close settings, or exit the app. The app verifies that Windows can read the saved key and reports an error if it cannot be saved. Once saved, the key is never displayed; use **Replace key** or **Remove saved key** in **Provider** to manage it. Existing keys saved by previous versions remain available. Windows protects saved credentials at rest, but software running as the same user or with sufficient system privileges may be able to retrieve them. Other preferences are saved in `%APPDATA%\Speeching\settings.json`. Audio and transcripts are processed by the provider you select; the model does not run locally.

## Conversation and controls

- **Interface language** in **Appearance** switches the app between English, Spanish, and Brazilian Portuguese. Save settings to keep the choice between launches. This is separate from the language being practiced, which is chosen in **Practice**.
- The voice selector describes each voice by perceived feminine or masculine register, tone where available, and a number that distinguishes similar voices. The provider's voice identifier is used internally and is not treated as the partner's name. Gemini's tone descriptions come from its voice catalog; OpenAI does not publish equivalent descriptions for its Realtime voices. Voice, language, provider, model, and endpoint apply to the next session.
- Search for a supported language in settings and choose a suggestion. The options follow the selected provider; custom endpoints and individual models can differ in language quality or availability. The lists are based on [Gemini Live language support](https://ai.google.dev/gemini-api/docs/live-api/capabilities#supported_languages) and [OpenAI's documented speech languages](https://developers.openai.com/api/docs/guides/text-to-speech#supported-languages).
- **Speaking pace** asks the model to change its delivery and also adjusts the streamed audio locally using pitch-preserving time stretching. The slower settings are more distinct. Changes apply to the next conversation. Extreme settings can introduce some audio artifacts or latency, but they do not deliberately change the speaker's pitch.
- **Your instructions** are appended to the language-practice system instruction sent to the provider when the next session starts. They are saved with your local preferences; avoid putting secrets there.
- Choose **Voice activated** for automatic turn detection or **Push to talk** to hold a shortcut while the main window is focused and release to send. Space is the default; select the shortcut in settings and press a new key with optional Ctrl, Alt, or Shift. The small microphone still mutes capture. The optional noise gate reduces sounds below the chosen threshold; adjust it for your room, since setting it too high can suppress quiet speech. It is a level gate and cannot distinguish loud background sounds from speech.
- Voice symbols and register labels are approximate descriptions of how a voice sounds; listeners may hear them differently.
- Short local sounds mark the beginning and end of a conversation, muting and unmuting the microphone, and pressing and releasing push to talk. The speaking indicator stays steady across brief pauses.
- Messages appear in a transparent, click-through bubble area above the main window. The partner begins as **AI Partner**; when an explicit self-introduction is recognized in the partner's transcript, the bubbles, main window, and log use that name. Detection is conservative and supports common introductions in English, Spanish, Portuguese, French, German, and Italian; other wording may remain **AI Partner**. Names are inferred locally from transcripts, with no additional API call. Messages move upward and fade. **Bubble lifetime** defaults to eight seconds and can be changed in settings; fading does not remove anything from the log.
- The main window shows a live conversation timer after connection. Each session in the log includes its duration and voice; the information is included in copies and downloads. The eye button hides conversation text, bubbles, and the timer for a text-free view. The full log remains available from the list button, with **Copy**, **Download**, and **Clear** actions.
- A new conversation is selected by default. Between sessions, the compact selector in the main window shows whether the microphone will start a new conversation or continue a saved one. Open it to choose **New conversation** or search saved chats by title, language, or transcript, then press the microphone. You can also choose chats in the log window. A selected saved conversation can be renamed or deleted between sessions; deletion asks for confirmation and removes its local file. A selected chat stays selected after you stop, so starting again continues it. **Clear** removes the selected chat's transcript from its saved file as well as from the log.
- The picker and saved-chat list show each conversation's last activity in local time. Choose newest, oldest, or title order; the choice is remembered. Automatic titles select a descriptive phrase from the transcript as the discussion develops, without an extra model request. Older automatic titles are refreshed for display; titles you rename are preserved. If a chat contains only greetings, it appears as **Practice conversation** until there is a more descriptive phrase.
- Conversations are saved as lightweight JSON in `conversations/` beside `speeching.exe`; audio and API keys are not stored in those files. The folder is excluded from Git, but the files are not encrypted. When continuing a chat, the recent transcript is sent to the selected provider as conversation history. Up to 120 messages and 48,000 characters are restored; older messages remain in the local log. Provider usage and charges can apply to restored context.
- Settings and the conversation log open as separate desktop windows that can be moved independently. Their main-window buttons toggle each window open and closed, and their positions are remembered in `%APPDATA%\Speeching\window-positions.json`. In **Appearance**, choose light or dark mode and optionally open the log automatically after a conversation ends or disconnects. The log option is off by default and does not open on a failed initial connection.

## Providers

Gemini Live and OpenAI Realtime have separate protocol adapters, keys, voices, models, and editable WebSocket URLs. A custom endpoint must implement the selected adapter's live protocol; an arbitrary provider URL alone is not enough. Remote endpoints must use `wss://`; an unencrypted `ws://` endpoint is allowed only on loopback for local development. The entered key is sent to the configured endpoint, so use an address you trust. Provider access, quotas, terms, and charges apply independently.

For Gemini, the default model is `gemini-3.8-live`. The app requests session resumption and tries to reconnect if the connection drops and Google supplied a resume handle. For OpenAI, the default is `gpt-realtime-2.1`; its Realtime API uses its own billing and model access. Neither adapter guarantees unlimited free use.

## Troubleshooting

- **No sound or microphone:** check Windows' default input/output devices and microphone permissions. Try headphones to prevent speaker echo.
- **Connection rejected:** check the selected protocol, WebSocket URL, model, API key, and provider quota or account access. Custom services must accept the same live-audio events as the selected adapter.
- **Bubbles disappear:** this is intentional; open the conversation log for the complete session. Increase Bubble lifetime if needed.
- **Old app:** `start-web.cmd` starts the complete original Node/browser version preserved in `legacy-web/`. The native version uses its own settings.

## Build from source

Install the current Rust toolchain and Windows build tools, then run `cargo test --locked` and `cargo build --release --locked`. The executable is produced at `target\release\speeching.exe`. To replace the ready-to-run copy, copy that executable to the project root. The original browser project is preserved in `legacy-web/`.

The project includes a small patched copy of `egui-winit` under `vendor/` so Windows respects the no-shadow setting on the transparent bubble window. It remains under the upstream MIT license.
