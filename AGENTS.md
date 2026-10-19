# AGENTS.md

HarshPaint follows the workspace's craft-app rules (`D:\Reeverse\.claude\skills\craft-app\SKILL.md` on the
author's machine); the essentials:

- **Layers:** `crates/core` (engine, no UI crates) ← `apps/harshpaint` (egui shell) and `apps/harshpaint-cli`.
- **Everything is a command:** add new actions to `crates/core/src/commands.rs` first; handle them in
  `Session::run` (engine) or `PaintApp::run` (dialogs, files, clipboard, view).
- **Never crash:** no `unwrap`/`expect`/`panic!`/indexing outside tests (workspace lints), `unsafe` is forbidden.
- **Tests are the gate:** `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` must pass; CI runs them on Linux, Windows and macOS.
- **Clean-room:** study behaviour only; never copy another product's code, icons or assets. Evidence goes in
  `docs/parity.md`.
- After a session, add a terse entry to `log/devlog.md`.
