# ![Ayin Launcher](/.github/assets/app_cover.png)

![Issues](https://img.shields.io/github/issues-raw/Ayinaki/AyinLauncher?color=c78aff&label=issues&style=for-the-badge)
![Pull Requests](https://img.shields.io/github/issues-pr-raw/Ayinaki/AyinLauncher?color=c78aff&label=PRs&style=for-the-badge)
![Contributors](https://img.shields.io/github/contributors/Ayinaki/AyinLauncher?color=c78aff&label=contributors&style=for-the-badge)
![Last Commit](https://img.shields.io/github/last-commit/Ayinaki/AyinLauncher?color=c78aff&label=last%20commit&style=for-the-badge)

## Ayin Launcher

Ayin Launcher is a desktop Minecraft launcher, personalized for private use by [Ayinaki](https://github.com/Ayinaki). It is built with [Tauri 2](https://v2.tauri.app/) and [Vue 3](https://vuejs.org/) on top of the [Theseus](https://github.com/modrinth/theseus) app core.

This is a personal project, not an officially distributed or supported product. There is no public website, support channel, or Discord at this time.

### What it does

- **Instance management** — create, launch, update, and switch versions of Minecraft instances, with per-instance settings and saves.
- **CurseForge catalog modpacks** — install curated modpacks from the built-in catalog with a live install progress modal, blocked-mod handling, and one-click version changes.
- **Minecraft color codes** — MOTDs and chat messages render proper formatting codes.
- **Imports** — bring instances over from MultiMC, PrismLauncher, GDLauncher, ATLauncher, and CurseForge.
- **Auto-updates** — releases are signed and served from this repository's Releases page.

## Installation

Pre-built installers are attached to every [GitHub Release](https://github.com/Ayinaki/AyinLauncher/releases/latest):

- **Windows** — `.exe` (NSIS installer)
- **macOS** — `.dmg`
- **Linux** — `.deb`, `.rpm`, `.AppImage`

The launcher's built-in updater checks the latest release, so after installing once, updates install themselves.

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) `>= 24.15.0`
- [pnpm](https://pnpm.io/) `10.x` (the repo pins `10.33.2`)
- [Rust](https://www.rust-lang.org/tools/install) `1.95.0` (pinned in `rust-toolchain.toml`)
- [Tauri v2 system dependencies](https://v2.tauri.app/start/prerequisites/) for your platform

### Setup

```bash
pnpm install
pnpm app:dev
```

`pnpm app:dev` starts the Tauri shell with the Vue frontend on hot-reload — edits to either side refresh the running app automatically.

### Useful commands

| Command          | What it does                                 |
| ---------------- | -------------------------------------------- |
| `pnpm app:dev`   | Run the app in development mode (hot reload) |
| `pnpm app:build` | Build a release bundle                       |
| `pnpm lint`      | Lint and format-check everything             |
| `pnpm test`      | Run the test suites                          |
| `pnpm fix`       | Auto-fix lint and formatting issues          |

### Repository layout

| Path                                           | Description                  |
| ---------------------------------------------- | ---------------------------- |
| `apps/app`                                     | Desktop app shell (Tauri)    |
| `apps/app-frontend`                            | Desktop app frontend (Vue 3) |
| `packages/app-lib`                             | Theseus core (Rust)          |
| `packages/ui`, `api-client`, `assets`, `utils` | Shared frontend packages     |

## CurseForge catalog setup

Installing CurseForge catalog modpacks requires a **CurseForge API key** — without one, installs fail with a clear error. The key is never committed to the repository.

1. Get a key from the [Overwolf CurseForge console](https://console.curseforge.com/) (third-party developer program).
2. Provide it one of three ways (resolved in this order at runtime):
   - copy `.env.example` to `.env` in the repo root and fill in `CURSEFORGE_API_KEY=...`, or
   - set a `CURSEFORGE_API_KEY` environment variable before running the app, or
   - set `CURSEFORGE_API_KEY` when building, so it is embedded into the binary at compile time.

The curated pack list lives in [`apps/app-frontend/src/assets/curseforge-packs.json`](apps/app-frontend/src/assets/curseforge-packs.json). The launcher fetches that file **live from this repository** at startup (with the bundled copy as a fallback), so adding a pack there is all it takes to publish it — no app release needed.

## Roadmap — working toward v1.5.0

Recent releases shipped the CurseForge catalog install pipeline (`v1.1.0`) and Minecraft color codes plus the live catalog fetch (`v1.2.0`). What's on the way:

- **Restore auto-updates (next release, `v1.3.0+`)** — the updater signing key was rotated, so the first release under the new key will need a one-time manual reinstall; updates work automatically afterwards.
- **Expand the CurseForge catalog** — more curated modpacks, added straight to `curseforge-packs.json`.
- **In-app CurseForge browsing/search** — pending an approved Overwolf API key.
- **The v1.5.0 feature set** — the wishlist is being scoped and prioritized; this section will be updated as items land.

## Contributing

This is a personal project maintained for individual use. Contributions are not actively sought, but you're welcome to open an issue if you spot something worth flagging.

## License

All packages in this repository are licensed under their respective licenses. See [COPYING.md](COPYING.md) for details.
