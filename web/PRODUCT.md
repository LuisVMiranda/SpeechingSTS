# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Language learners who want a natural spoken conversation from a browser on Linux, macOS, or Windows. The web service may serve multiple users with private conversation histories.

## Product purpose

Provide speech-to-speech language practice through live AI providers, with a compact conversation-focused interface and reusable conversation history.

## Operating context

The Rust web service is deployed independently, including on a small VPS. Browser microphone access requires a secure origin. The existing Windows application, credentials, configuration, and conversations remain a separate environment.

## Capabilities and constraints

- Support Gemini Live and OpenAI Realtime where configured, without embedding long-lived provider keys in browser assets.
- Keep each authenticated user's conversations and provider credentials separate.
- Support built-in authentication and a trusted upstream authentication gateway; authentication may be disabled deliberately for a private single-user setup.
- Apply a configurable cooldown after three failed sign-in attempts, and record security incidents without recording passwords, API keys, audio, or private transcripts.
- Keep the service lightweight, with a target of less than 100 MiB of service memory for a small deployment; this is a target to measure, not a guaranteed result.
- Preserve the existing compact, contemporary Speeching identity, practice controls, accessible interaction, and English, Spanish, and Brazilian Portuguese interface options where feasible.
- Treat transcripts, restored history, and model output as untrusted data; the model must not gain server tools or credentials.

## Brand commitments

Speeching uses its existing blue microphone and restrained light and dark visual language. The conversation remains the primary focus.

## Evidence on hand

The existing Rust desktop application and preserved `legacy-web` browser prototype provide provider protocols, audio behavior, icons, and established visual references. They are not production web deployments.
