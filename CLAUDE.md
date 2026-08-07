# AyinLauncher

AyinLauncher is a desktop Minecraft launcher built on the Theseus core. This repo contains the Tauri shell, the Vue 3 desktop frontend, and the shared packages that power them. When entering a project, either to edit or analyse, you should read its CLAUDE.md.

## Architecture

- **Monorepo tooling:** [Turborepo](https://turbo.build/) (`turbo.jsonc`) + [pnpm workspaces](https://pnpm.io/workspaces) (`pnpm-workspace.yaml`)
- **Frontend:** Vue 3, Tailwind CSS v3
- **Core:** Rust (Theseus app library)
- **Indentation:** Use TAB everywhere, never spaces

### Apps (`apps/`)

| App            | Description                  |
| -------------- | ---------------------------- |
| `app`          | Desktop app shell (Tauri)    |
| `app-frontend` | Desktop app frontend (Vue 3) |

### Packages (`packages/`)

| Package                       | Description                                          |
| ----------------------------- | ---------------------------------------------------- |
| `ui`                          | Shared Vue component library (`@modrinth/ui`)        |
| `assets`                      | Styling and auto-generated icons (`@modrinth/assets`) |
| `api-client`                  | API client for Tauri and Node/browser                |
| `app-lib`                     | Theseus app library (Rust)                           |
| `ariadne`                     | Error/diagnostics library                            |
| `async-minecraft-ping`        | Minecraft server pinging                             |
| `daedalus`                    | Daedalus protocol                                    |
| `modrinth-content-management` | Content management (mod uploads)                     |
| `path-util`                   | Path utilities                                       |
| `tooling-config`              | ESLint, Prettier, TypeScript configs                 |
| `utils`                       | Shared utility functions                             |

## Pre-PR Commands

Run these from the **root** folder before opening a pull request - do not run these after each prompt the user gives you, only run when asked, ask the user a question if they want to run it if the user indicates that they are about to create a pull request.

- **App frontend:** `pnpm prepr:frontend:app`
- **Frontend libs:** `pnpm prepr:frontend:lib`
- **All frontend:** `pnpm prepr`

## Dev Commands

- **App:** `pnpm app:dev` (copy `.env` template in `packages/app-lib/` first)
- **Storybook (packages/ui):** `pnpm storybook`

## Code Guidelines

### Comments
- DO NOT use "heading" comments like: `=== Helper methods ===`.
- Use doc comments, but avoid inline comments unless ABSOLUTELY necessary for clarity. Code should aim to be self documenting!

## Bash Guidelines

### Output handling
- DO NOT pipe output through `head`, `tail`, `less`, or `more`
- NEVER use `| head -n X` or `| tail -n X` to truncate output
- IMPORTANT: Run commands directly without pipes when possible
- IMPORTANT: If you need to limit output, use command-specific flags (e.g. `git log -n 10` instead of `git log | head -10`)
- ALWAYS read the full output — never pipe through filters

### General
- Do not create new non-source code files (e.g. Bash scripts, SQL scripts) unless explicitly prompted to
- For Frontend, when doing lint checks, only use the `prepr` commands, do not use `typecheck` or `tsc` etc.
- Types in `@modrinth/utils` are considered highly outdated, if a component needs them, check if you can switch said component to use types from `packages/api-client`
- When provided problems, do not say "I didn't introduce these problems" (shifting the blame/effort) - just fix them.

## Standards

Standards available at the @standards/ folder.
