# AGENTS.md — IslandWin

IslandWin is a Tauri 2 + React/TypeScript dynamic-island overlay for Windows. Rust lives in
`src-tauri/src`, the UI in `src/`. The overlay is a small transparent window that follows the island;
`src-tauri/src/main.rs` owns placement, DPI and hit-testing.

## Build and test

```sh
npm install
npm run typecheck        # tsc --noEmit
npm run build            # tsc + Vite production build
node scripts/design-check.mjs
cd src-tauri
cargo check --all-targets --locked
cargo test --locked
npm run package          # signed NSIS installer (see docs/SIGNING.md)
npm run verify:signatures
```

CI (`.github/workflows/ci.yml`) runs typecheck, build, design checks, `cargo check`, `cargo test`
and a secret scan.

## Conventions

- Keep the island compact. `PAGE_H` in `src/App.tsx`, `BOX_MAX_W`/`BOX_MAX_H`, and the Rust
  `BOX_W`/`BOX_H` in `src-tauri/src/main.rs` plus the window size in `src-tauri/tauri.conf.json`
  must stay in sync.
- UI preferences persist in `localStorage` as `on` / `off`; read them in the state initializer and
  write them in a `set*Pref` helper.
- The design check rejects `!important`, emoji in the UI, off-palette hex colors, stray easing
  curves and `steps()` animations. Use the system palette and the existing motion springs.
- Match the surrounding style and line endings; do not reformat unrelated code.
