# Notor

Desktop productivity app that surfaces Google Calendar events and popup reminder alerts from the system tray.

Notor signs in with Google, watches your calendars, and fires native alert windows when popup reminders are due — including join links for Google Meet and Zoom when an event has them.

**Live web UI:** [notor.vercel.app](https://notor.vercel.app)

## Features

- System tray desktop app for upcoming calendar events
- Google Calendar OAuth sign-in (multi-account)
- Alerts based on Google Calendar popup reminders
- Show/hide calendars per account
- Autostart and auto-update support via Tauri

## Stack

| Layer | Path | Tech |
| --- | --- | --- |
| Desktop shell | `src-tauri/` | [Tauri](https://tauri.app/) (Rust) |
| UI | `src/` | [Next.js](https://nextjs.org/) 14, React, Redux, Tailwind CSS |
| Auth API | `src-server/` | Actix Web (Rust) — Google OAuth for calendar access |

The Next.js app is statically exported and embedded by Tauri. In production, the OAuth server runs separately (e.g. DigitalOcean App Platform).

## Prerequisites

- [Node.js](https://nodejs.org/) 26.x
- [Yarn](https://yarnpkg.com/) (lockfile is Yarn)
- [Rust](https://rustup.rs/) toolchain
- [Tauri prerequisites](https://tauri.app/v1/guides/getting-started/prerequisites) for your OS
- A Google Cloud OAuth client with Calendar API access (for local auth)

## Getting started

### Install UI dependencies

```bash
yarn install
```

### Run the auth API (required for Google sign-in)

```bash
cp src-server/.env.example src-server/.env
# fill in Google OAuth + Calendar API values
cd src-server
cargo run
```

By default the local API listens for the UI at `http://127.0.0.1:4876`.

### Run the desktop app

```bash
# from the repo root
yarn tauri
```

This starts the Next.js dev server and the Tauri window.

### UI only (browser)

```bash
yarn dev
```

Open [http://localhost:3000](http://localhost:3000). Browser-only mode still needs the auth API for Google login; native tray/alert behavior requires Tauri.

### Build

```bash
yarn build          # Next.js static export
yarn build:app      # Tauri desktop package
```

## Project layout

```
src/           Next.js UI (tray, alerts, sign-in)
src-tauri/     Tauri shell, reminder policy, system tray, notifications
src-server/    Google OAuth Actix service
```

## License

MIT — see [LICENSE](LICENSE).
