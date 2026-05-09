# Ritual

Ritual is a local desktop automation app built with Tauri, React, TypeScript, and Rust. It uses Claude through the Anthropic API to understand your requests, call local tools, and help operate your computer.

## Get Started With Claude Code

The easiest way to get this running is to let Claude Code walk you through it.

Once Claude Code is working on your Mac, paste this:

```text
Hi Claude.

Clone https://github.com/Anushlinux/ritual repository into my current directory.

Then read the README.md and AGENTS.md. I want to get Ritual running locally on my computer with my own Anthropic API key.

Help me install the required tools, run the app, add my Anthropic API key in Settings, and fix any setup errors you see.
Walk me through it step by step.
```

That is the friendliest path for non-technical users. Claude Code can check what is already installed, explain each step, and help recover if Node, Rust, or system permissions are missing.

## Important: Bring Your Own API Key

This app does **not** include anyone else's Claude API key. If someone sends you the app or this repo, you still need your own Anthropic API key.

Do not hardcode a private API key into a shared build. Desktop app secrets can be extracted. If you want a one-click public app where users do not add a key, run your own backend proxy and keep the API key on the server.

## Install and Use a Release Build

1. Download the latest build from the GitHub Releases page.
2. Unzip it and move `imprint.app` to Applications.
3. Open the app.
4. Click the settings dots, paste your Anthropic API key in **Claude API key**, and click **Save**.
5. Try a simple prompt like `List the files in my current project folder.`

The saved key lives only on your computer in the app config folder. It is not committed to the repo and is not included in shared builds.

## Manual Developer Setup

### Prerequisites

- A Mac, Windows, or Linux computer
- Node.js 18+
- Rust and Cargo from `rustup`
- An Anthropic API key

For macOS desktop control features, you may also need to allow Accessibility, Screen Recording, and Microphone permissions when the app asks.

### 1. Install Dependencies

```bash
npm install
```

### 2. Run the Desktop App

```bash
npm run tauri dev
```

This starts the Vite frontend and the Tauri desktop shell.

### 3. Add Your Claude API Key

The easiest path is inside the app: open Settings, paste your Anthropic API key in **Claude API key**, and click **Save**.

For development, you can also use an env file instead:

Copy the example env file:

```bash
cp src-tauri/.env.example src-tauri/.env
```

Open `src-tauri/.env` and set:

```env
ANTHROPIC_API_KEY=your_api_key_here
ANTHROPIC_MODEL=claude-sonnet-4-5-20250929
```

`ANTHROPIC_MODEL` is optional. The default is Claude Sonnet 4.5.

### 4. Try a Simple Prompt

When the app opens, try something low-risk first:

```text
List the files in my current project folder.
```

Then try a normal chat question. For file edits or dangerous operations, the app should ask for approval before continuing.

## Optional Features

### Voice Transcription

Voice transcription uses Google Cloud Speech-to-Text. It is optional. If you want it, add a service account JSON file and set these in `src-tauri/.env`:

```env
GCP_SERVICE_ACCOUNT_KEY_PATH=keys.json
GCP_STT_LANGUAGE_CODE=en-US
GCP_STT_MODEL=latest_short
```

### Connectors

GitHub and Google Workspace connector tokens can be added in `src-tauri/.env`:

```env
GITHUB_TOKEN=
GOOGLE_ACCESS_TOKEN=
```

The in-app OAuth callback flow is not wired yet, so env tokens are the current setup path.

## Build

To check the Rust backend:

```bash
cd src-tauri
cargo check
cargo test
```

To build the frontend:

```bash
npm run build
```

To create a production Tauri bundle:

```bash
npm run tauri build
```

## Project Structure

- `src/` - React frontend, UI components, hooks, stores, and styles
- `src-tauri/src/` - Rust backend, Claude runtime, tools, browser automation, safety checks, and connectors
- `src-tauri/.env.example` - template for API keys and local configuration
- `src-tauri/assets/`, `src-tauri/icons/`, `public/` - fonts, icons, and static assets

## Troubleshooting

If the app says `ANTHROPIC_API_KEY is missing`, open Settings, paste your Anthropic API key under **Claude API key**, and click **Save**. Developers can also check that `src-tauri/.env` exists and contains the key.

If desktop control does not work on macOS, open System Settings and grant the requested permissions for the app or terminal you launched it from.

If setup feels confusing, use the Claude Code prompt at the top of this README and let it guide you through the install.
