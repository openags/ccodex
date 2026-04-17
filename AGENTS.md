# AGENTS.md

## Execution Preference

- Keep executing the project plan continuously.
- Do not stop after each small milestone to ask whether to continue.
- Prefer making the next reasonable implementation decision and moving forward.
- Use short progress updates, but avoid blocking on confirmation unless there is a real conflict or blocker.

## Project Guidance

- Keep `ccodex` aligned with `temp_files/repo_analysis/ccodex_master_plan.md`.
- Preserve the shared architecture:
  - `modules/protocol`
  - `modules/kernel`
  - `modules/runtime`
  - `modules/store`
  - `apps/cli`
  - `apps/tui`
  - `apps/local-server`
- Prefer incremental implementation over large rewrites now that the bootstrap vertical slice is working.
- Keep local runtime data inside project-local `.ccodex/` by default.
- Claude-compatible behavior is the main parity target; match semantics before matching polish.
