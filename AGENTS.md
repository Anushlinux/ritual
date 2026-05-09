# Repository Guidelines

## Project Structure & Module Organization

This is a Tauri desktop app with a Vite, React, and TypeScript frontend plus a Rust backend.

- `src/` contains the frontend app, styles, stores, hooks, and components.
- `src/components/` uses PascalCase React component files.
- `src/hooks/` contains `use*` hooks; `src/store/` and `src/stores/` hold client state.
- `src-tauri/src/` contains Rust backend modules, Tauri commands, browser automation, safety, tools, and shared types.
- `src-tauri/assets/`, `src-tauri/icons/`, and `public/` contain fonts, app icons, and static assets.

## Build, Test, and Development Commands

- `npm install` installs frontend and Tauri CLI dependencies.
- `npm run dev` starts the Vite frontend.
- `npm run tauri dev` runs the full desktop app with the Rust backend and Vite frontend.
- `npm run build` type-checks TypeScript with `tsc` and builds the Vite frontend.
- `npm run tauri build` creates a production Tauri bundle.
- `cd src-tauri && cargo check` validates Rust code quickly.
- `cd src-tauri && cargo fmt` formats Rust code.
- `cd src-tauri && cargo test` runs Rust tests when present.

## Coding Style & Naming Conventions

TypeScript is strict: `noUnusedLocals`, `noUnusedParameters`, and switch fallthrough checks are enabled in `tsconfig.json`. Keep React components in PascalCase, hooks named `useThing`, and shared types in explicit `type` or `interface` definitions. Follow nearby import and styling patterns.

Rust code should follow `rustfmt` defaults. Keep Tauri command payloads serializable with `serde`, return `Result<_, String>` where existing commands do so, and place browser or safety logic in the existing modules.

## Testing Guidelines

There is no frontend test runner configured yet. For frontend changes, run `npm run build` at minimum. For Rust/backend changes, run `cargo check` and `cargo test` from `src-tauri/`. Add tests near code that introduces parsing, safety rules, command behavior, or state transitions.

## Commit & Pull Request Guidelines

Git history includes both `feat: scaffold UI architecture...` and a plain setup commit. Prefer short, imperative Conventional Commit messages such as `feat: add permission review panel` or `fix: handle missing service account key`.

Pull requests should describe the user-facing change, list verification commands run, mention skipped tests, and include screenshots or screen recordings for UI changes. Link related issues when available.

## Security & Configuration Tips

Do not commit real `.env` files, service account JSON, API keys, or generated bundles. Use `src-tauri/.env.example` as the template for `ANTHROPIC_API_KEY`, optional `ANTHROPIC_MODEL`, `GCP_SERVICE_ACCOUNT_KEY_PATH`, and optional Speech-to-Text settings.
