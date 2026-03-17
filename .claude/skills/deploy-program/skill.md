---
description: Deploy a LEZ program to the sequencer. Use when the user asks to deploy, ship, or publish a program (e.g. "deploy the token program", "ship amm to the sequencer").
---

# deploy-program

Deploying a LEZ program is always a two-step process: compile first, then deploy. Never deploy
without rebuilding first — a stale binary deploys silently but won't reflect recent code changes.

The program name corresponds to a top-level workspace directory. If none is specified, discover
available programs by looking for `<name>/methods/guest/Cargo.toml` and ask the user to pick one.

After deploying, confirm success by inspecting the binary and reporting the ProgramId to the user.

## Gotchas

- **Docker must be running.** `cargo risczero build` cross-compiles via Docker. Fail fast if not.
- **The output binary path follows a fixed convention** — derive it from the program name, don't guess.
