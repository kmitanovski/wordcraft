# Document tabs verification

> **Last reviewed:** 2026-10-26 · **Last updated:** 2026-10-26 · **Change:** minor (PR review regressions and workspace ownership)

Implementation and source revisions: [document-tabs.md](document-tabs.md).

## Quality gate

`cargo xtask ci` passed all eight stages locally: formatting, workspace Clippy with
warnings denied, workspace tests, translation audit, doctests, asset attribution,
dependency layering and the 16-crate WASM check. The workspace test stage passed
4395 tests after upstream reconciliation (4196 on the original integration base). Existing ignored benchmarks remain ignored.
The translation audit passed; the detailed missing-translation report was unavailable
because this local build has no `CRAFT_I18N_DIR` catalog checkout.

Existing Rust 1.95 Clippy findings were fixed with equivalent boolean expressions and
match guards in a separate prerequisite commit; the quality bar was not lowered.

The publishing integration reconciles upstream `a029d8ae` (including egui 0.37), keeps
body-replacement IME tracking, and tests that tab switches interrupt an unfinished
composition without keeping its pre-edit in the inactive tab. Update restart eligibility
also checks inactive documents, with a regression for an inactive unsaved draft.

## Completed checks

- Native macOS Rust build and headless egui interactions, including independent undo/redo,
  selection, zoom, AutoSave authority, pinning, reordering, reopening and workspace restoration.
- Regression tests for inactive failed/cancelled browser saves, source-bound Save As,
  stale picture picks, stable tab IDs, malformed indexes, pending close prompts, repeated
  OS close requests and automation Quit with inactive unsaved work.
- Recovery from the Start placeholder, cancellation when typing begins before a recovery
  scan completes, the 128-entry workspace limit, and automatic selected-tab visibility in
  a narrow overflowing strip all have regression tests.
- A subprocess creates two inactive synthetic drafts and a password-protected draft in an
  isolated recovery directory. The parent waits for two snapshots, terminates the process,
  discovers both drafts, and restores one into a new unsaved tab while preserving current
  writing. The encrypted document leaves no plaintext snapshot. Test duration: about 10 seconds.
- Headless screenshots inspected at 1440×900 and 780×600 with nine long document names,
  pinning, an active tab and unsaved indicators. The strip scrolls horizontally; the narrow
  ribbon remains responsive. Review artifacts are local under `/tmp/wordcraft-tabs-review/`.

## PR #4359 review follow-up

`cargo xtask ci` passed all eight stages again after these fixes, with 4401 workspace
tests passing. Five new UI regressions and one desktop preferences regression were added.

All six inline CodeRabbit findings and the additional M8 roadmap finding were verified
against the published head. Regression coverage adds: Start disabled with an empty
workspace; exactly one all-tabs dirty notification; ordinary dialog dismissal on tab
switch and Close All with color-state cleanup; save/password/encryption/recovery
confirmations continuing to block switches; Enter leaving a disabled-only command
search open; and a secondary window retaining the primary window's latest persisted
workspace. Invalid preferences prevent the secondary write rather than replacing them.
The context-menu label now uses the translation lookup; translations still require the
external catalogs. M8 now names the remaining window commands individually.

## Native memory and control probe

The pre-egui-0.37 native macOS build used an isolated `WORDCRAFT_PROFILE_DIR` and localhost
control channel. Switch by stable ID, pinning, guarded Quit, Close All and the final
empty-window state passed. Explicit New Window launched a separate process with a blank
document. Only synthetic processes and documents were used. An app-window screenshot
was inspected in the dark system theme. After the final recovery and overflow fixes, a
rebuilt native app repeated these checks and ten open/edit/close cycles with exit code 0
(`native-final-smoke.json`). After reconciling upstream egui 0.37, the standalone native
app was rebuilt and repeated the same switch/pin/guarded-Quit/Close-All/New-Window checks
and ten cycles, again with exit code 0; its fresh app-window screenshot was inspected.
The historical 1,200-cycle samples below were not rerun on egui 0.37.

A 1,200-cycle rapid open/edit/close probe completed without command errors. Every closed
document disappeared from the tab list. RSS samples (KiB) at 0/100/200/300/400/500/600/700/
800/900/1000/1100/1200 cycles were:

`241280 / 269120 / 273456 / 277072 / 282560 / 285712 / 288096 / 290112 / 243184 / 244704 / 246080 / 247280 / 249088`.

RSS remained 249088 KiB after two idle seconds: about 7.6 MiB above the starting point,
after a peak sample of about 283 MiB. A separate 300-cycle run ranged
from 162640 to 208448 KiB. These samples fluctuate with allocation and reclamation;
they do not prove absence of a leak. No sustained linear growth over the full 1,200 cycles
was observed. This is a short control-channel stress check, not a long manual-editing soak.

A 300-cycle headless GPU-harness probe also completed without command errors but used
higher transient RSS (136688 KiB initially, 1506720 KiB peak sample, 1048096 KiB finally).
The native comparison did not reproduce that scale of growth, suggesting harness/GPU
allocation retention rather than equivalent live-document retention; heap profiling has
not confirmed that inference. Extended profiling remains separate.

Local machine-readable samples are under `/tmp/wordcraft-tabs-review/` in
`native-results.json`, `native-results-300.json` and `memory-results.json`.

## Platform limits

Windows and Linux native behavior have not been executed locally. A wasm compile checks
web compatibility but does not establish browser end-to-end or cross-platform readiness.
Real screen-reader, Finder-event and separate-window conflict field testing remain separate.

The sibling `../../craftrules/AGENTS.md` was absent. Attempts to read the referenced GitHub
resource returned 404; this work follows the repository's supplied clean-room, never-crash,
asset, layering and quality-gate requirements without claiming that unavailable file was read.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-26 | minor | PR review regressions cover secondary-window workspace ownership, Start disabled, modal scope, search and dirty callbacks |
| 2026-10-26 | major | Record tab safety regressions, process-termination recovery and screenshot inspection |
