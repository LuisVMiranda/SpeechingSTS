# Speeching

A compact, local language-practice window powered by Gemini 3.8 Live. Pick a voice and a language, then talk naturally. The waveform follows the microphone and the model's audio; both speakers' transcripts appear in the conversation.

## Start on Windows

1. Install [Node.js 20 or newer](https://nodejs.org/) if it is not already installed. No npm packages are required.
2. Double-click `start.cmd`. Leave its console window open while using the app. Edge or Chrome should open in a small app window; if neither is found, open the address printed in the console in a current browser.
3. In settings, paste a [Gemini API key from Google AI Studio](https://aistudio.google.com/app/apikey), choose the language and level, and save. Select a voice and press **Start conversation**. Allow microphone access when the browser asks.
4. Speak to begin. You can interrupt, mute, end the session, or clear the visible transcript. Headphones help prevent speaker echo. Close the console window to stop the local server.

The default language is English and the default level is intermediate. Enter any supported language in settings. Language, level, and voice changes apply to the next conversation.

## Privacy and usage

- The interface and local server run on your machine. Audio, transcripts, and the conversation are processed by Google's servers; this is not an offline model. Google may use free-tier content to improve its products; see [Google's pricing](https://ai.google.dev/gemini-api/docs/pricing) and [terms](https://ai.google.dev/gemini-api/terms).
- The API key is held in this window's memory, is sent to the local server to obtain a short-lived Google token, and is not saved by the app. Paste it again after reopening the window. Language, level, and voice are saved in browser storage. Conversation text remains visible until cleared or the window closes and is not saved by the app.
- Your Google account must have access to `gemini-3.8-live`. Google's quota, availability, terms, and any charges still apply. This app does not grant unlimited usage. Check your project's limits in AI Studio.
- Transcripts and corrections can be imperfect. The waveform is a visual indication of detected audio rather than a pronunciation score.

## Troubleshooting

- **Nothing opens:** use the address printed in the console in Chrome or Edge. Keep the console open.
- **Microphone blocked:** permit microphone access for the local address in the browser's site settings, then start again.
- **Google rejects the session:** check the key, model availability, project quota, and account eligibility in AI Studio. Try again after any rate limit resets.
- **Audio echo or interruptions:** try headphones or lower the speaker volume.
- **Long sessions:** the app requests context compression and attempts to resume connections when Google closes them. If a resumption fails or the token expires, start a new session; older context may no longer be available.

## Development

Run `npm start` (or `node server.mjs`) and `npm test`. Add `--no-open` to `node server.mjs` to launch without opening a browser. The local HTTP server binds only to `127.0.0.1` on an automatically assigned port. It obtains a short-lived token from Google; the browser then streams audio directly to the Live API over WebSocket.
