# Notor

Desktop meeting companion that starts from your calendar and is growing into an AI note taker for meetings, connected to the tools your team already works in.

Today, Notor signs in with Google, watches your calendars, and fires native alert windows when popup reminders are due, including join links for Google Meet and Zoom when an event has them. See the [Roadmap](#roadmap) for where the project is headed.

**Live web UI:** [notor.vercel.app](https://notor.vercel.app)

## Vision

Notor aims to be the app that knows what is on your calendar, helps you through the meeting, and carries the outcome into the places where work happens.

- **Before the meeting:** timely alerts, join links, and context pulled from the calendar event and connected apps.
- **During the meeting:** an AI note taker that works on your desktop, in the style of [Granola](https://www.granola.ai/). It captures audio from your device, so no bot has to join the call, and it blends your own quick notes with the transcript.
- **After the meeting:** enhanced notes, summaries, decisions, and action items that you can search, edit, and share to the tools your team uses.

Notor is meant for individuals first, then for teams who want shared meeting knowledge.

## Features (available today)

- System tray desktop app for upcoming calendar events
- Google Calendar OAuth sign-in (multi-account)
- Alerts based on Google Calendar popup reminders
- Show/hide calendars per account
- Autostart and auto-update support via Tauri

## Roadmap

Everything in this section is planned and not yet implemented. Scope and order may change.

### 1. AI meeting notes

- Local audio capture from the system (microphone and system audio) with no meeting bot
- Live and post-meeting transcription
- Notes editor where you jot down points and AI expands them into structured notes
- Automatic summaries, key decisions, and action items
- Meeting templates, such as 1:1, standup, interview, and customer call
- Searchable meeting history and an "ask across my meetings" chat
- Privacy controls: clear recording indicators, local-first storage where possible, and configurable retention and model providers

### 2. MCP integrations for workplace apps

Notor will support the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) in two directions:

- **As an MCP client:** connect to MCP servers for workplace apps to pull context into meetings and push results out. Planned targets include Slack, Discord, Microsoft Teams, Notion, Linear, Jira, GitHub, Google Drive, and email.
- **As an MCP server:** expose your meeting notes, transcripts, and action items so other AI tools and agents can use them.

Example workflows:

- Post a meeting summary and action items to a Slack channel or Discord server.
- Create Linear or Jira issues from action items.
- Pull the relevant thread, doc, or ticket into the notes before a meeting.
- Let an AI assistant answer "what did we decide about X last week?" from your meeting history.

### 3. Individuals and teams

- **Individuals:** personal workspace, bring-your-own-account connections, and per-integration permissions you can turn on or off.
- **Teams:** shared workspaces and note folders, team-level integration connections, role-based access, and shared templates.
- Explicit consent and sharing controls for notes, with audit-friendly access to connected apps.

### 4. Calendar and platform foundation

- More calendar providers beyond Google Calendar, such as Outlook
- Better meeting detection for Meet, Zoom, Teams, and other providers
- Sync across devices and a stable web experience
- Plugin and automation hooks for custom workflows

## Stack

The current stack is below. Roadmap features will add components for audio capture, transcription and summarization, and MCP connections.

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

## Contributing

Ideas and feedback on the roadmap are welcome. Open an issue to discuss an integration, workflow, or privacy requirement before starting large changes.

## License

MIT — see [LICENSE](LICENSE).
