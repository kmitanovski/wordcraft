# Document tabs verification

> **Last reviewed:** 2026-10-26 · **Last updated:** 2026-10-26 · **Change:** minor (latest full-diff review fixes)

Implementation and source revisions: [document-tabs.md](document-tabs.md).

## Follow-up review gate

Before pushing a remediation, fetch current CodeRabbit reviews and verify each finding
against the code, reproduce confirmed defects, then run a local `codex review` of the
PR and final remediation diff. Resolve confirmed review findings, run the affected tests
and the full `cargo xtask ci`, then inspect and commit the final diff. A passing build
alone does not satisfy this review gate.

## PR #4359 review follow-up

This pass uses verified upstream `main` at `2c99a427803364354181d0bc20865f05880605f7`.
Codex review found three edge cases: closing a document also open in another window's
inactive tab could remove it from both windows; duplicate Open was checked after the
128-tab limit; and a secondary process could merge a stale workspace during a concurrent
preference write. The window lookup now matches the actual window and includes inactive
tabs, duplicate lookup precedes the cap, and preference read/merge/write is protected by a
cross-process file lock. Regressions cover each case.

The fresh local CodeRabbit review covered all 48 changed paths and found a `.get(0)` Clippy
warning and a malformed revision-history table. Both are corrected. Subsequent Codex review
found that direct `file.closeDocument` calls and addressed control-channel closes omitted
inactive comment drafts and unconfirmed saves; close checks now use per-document state.
CodeRabbit also found that Command Search enabled unavailable tab actions and discarded
command errors; state-based enablement and visible errors are covered by regressions.
Workspace preferences now mark non-Unicode paths and report them instead of restoring a
lossy path. Closing a parked document stops Read Aloud when it belongs to that document.
Focused regressions and local review coverage are recorded after the final gate below.

CodeRabbit's retained reliability concern was reproduced: an explicit forced close
during Quit removed the prompt but stranded the Quit-review latch. The shared tab-close
path now cancels that interrupted review. A regression covers both window-close and
Close All retries after one previously reviewed tab, checks that remaining dirty work
is prompted again, and checks that Cancel preserves it. The regression failed before
the production fix.

The first local Codex code review inspected the full PR against `a029d8ae` and found
three additional defects. Each was reproduced before fixing it: newly created sessions
lost the custom-dictionary persistence path and effective editing language, and tab
shortcuts consumed keys intended for Customize Keyboard. The shared preferences helper
now propagates the path and effective language; keyboard recording returns before tab
shortcut dispatch. Regressions write and read a temporary dictionary file, cover Polish
and Australian English in New/TXT/Markdown tabs while preserving existing document
language, and verify forward/reverse cycling and reopen keys remain available to record.

CodeRabbit's completed second pass identified a further browser picker ownership issue.
Two regressions reproduced a late recipient result adopting another tab's owner and an
unrelated drop being discarded by a stale picker token. Browser pickers now use one
request-specific channel carrying a complete batch, separate from the drop/host inbox.
As on desktop, a pending picker refuses a second picker with visible feedback. Empty
or disconnected replies cancel cleanly. The operation retains its purpose and document;
picture/recipient batches cancel if their document is no longer active. Regressions
cover old and extra replies, cancellation/disconnection, drops during a pending request,
multiple pictures, and accepting a later valid request. Existing font/recipient tests
now exercise the result channel.

CodeRabbit's generic docstring coverage result is inconclusive and is not a repository
CI requirement. The generic percentage alone does not demonstrate a functional defect;
this change does not add redundant comments merely to satisfy that count.

The earlier local Codex review checked the complete remediation diff and relevant callers
against the specification and repository rules, including the browser host callback.
It reported no actionable defects at that point. That was a source review; test/build results are
recorded separately below. Eight new regressions cover the confirmed issues.

A later Codex review found a Quit race: an addressed control command could edit a
document already reviewed in a multi-document quit flow. Quit now records each reviewed
document revision and asks again if that revision changes. It also found that opening a
file owned by another native window could append it to a full current tab strip; Open now
routes through the existing window handoff. Regression tests exercise both cases, including
the 128-tab cap.

The next Codex review found that pruning a closed document from an inactive window could
reuse that document's parked view for another tab. Pruning now drops the stale view; the
regression checks the replacement tab keeps its own zoom. It also found that Copy Full Path
could place a lossy replacement name on the clipboard for a native non-Unicode path; such
paths now produce no copy action. Both regressions pass.

## Unreadable-file follow-up

PR comment `discussion_r4373185896` is confirmed: a browser File read rejection
previously sent empty bytes to Open. The production batch reader now skips that
file, queues a translated status message naming it, and retains genuine empty
files and other readable files in order. The request still completes with an
empty batch when all reads fail, allowing the next picker request.

The WASM `picker_check` harness exports `read_files(files, many)` to exercise the
production reader with real browser Files. Inject a rejecting `arrayBuffer` on
one File and verify its name is absent from results and present in messages;
also check valid empty files, mixed batches, later successful reads and the
2,000-file cap. This verifies the browser API boundary without changing host
credentials or opening native OS dialogs.

The local CodeRabbit setup review also identified four minor issues: closing
during a hidden file/Zotero operation now reports why it is blocked; Command
Search excludes tab inspection and commands requiring parameters from both
click and Enter results; duplicate visual-diff DPI options are rejected before
creating output; and the duplicate AutoCorrect revision row is removed. Rust
regressions cover the three behavior changes. Readiness estimates are unchanged.

The next local review found that File › Open hides the editor status bar, so the
Open page now renders the read failure itself. A headless UI regression completes
an empty browser result and verifies that the named failure is visible, no tab
was opened, and the picker is released. Tab search also matches translated labels
with the existing locale-aware casing. A failed inactive-tab Save As retains its
feedback after returning to the previously active tab; regressions cover Turkish
search and an unwritable destination while preserving the dirty document.

Further review identified two lifecycle gaps: dismissing Save Changes through its
title-bar close box now answers Cancel, clearing Quit and Close All queues while
preserving the writing. A headless pointer regression retries both operations.
Font changes now invalidate inactive document layouts as well as the active one;
a cached-layout regression verifies the next tab switch recomputes it.

Tab shortcuts also preserve unapplied dialog edits even when no dialog field is
focused. Cancelling an inactive document's non-save picker returns cancellation
without a spurious failure. A busy existing picker cancels an attempted Quit
save sequence so later Quit can prompt again. Background Save As also restores
the visible tab's dialog draft, palette, Backstage and focus request. Regressions
cover these cases. Attribution keeps contributor
names and source revisions without repeating their email addresses in this guide.
If every saved workspace path fails, restoration preserves the initial blank;
launch then applies the Start preference. Both preference values are covered.
An inactive Save As result remains in its native result channel while the active
document has a confirmation modal; the built-in save browser also waits. Closing
the modal resumes the original save without losing its selected destination. A
regression checks that the file is saved to the original tab after cancellation
of the active tab's close prompt, leaving that active tab dirty and selected.

## Final review follow-up

CodeRabbit and Codex identified four further defects: an inactive picture picker left
a stale replacement target; workspace restore errors were hidden on Home; multiple
unreadable-file notices overwrote each other; and visual-diff silently ignored
misspelled options. A subsequent review also found that pending exports counted as
unsaved document work. The fixes clear targets on their owning tab, render startup
errors on Home, join each batch of messages, reject unknown flags before filesystem
changes, and count only lossless document saves as unconfirmed work. Targeted
regressions cover each behavior.

## Browser dismissal follow-up

The open CodeRabbit comment `discussion_r4373049046` is confirmed: rfd's browser
picker can remain unresolved when dismissal produces neither change nor cancel.
The web host now uses a request-owned native file input. Change/cancel complete it
normally; the next page pointerdown/keydown is a fallback after silent dismissal.
The fallback reads the selected FileList rather than assuming cancellation, so a
late change event cannot discard selected files. No elapsed-time timeout can cancel
an unhurried selection. Dropping the selection removes the input and all listeners.
When transient user activation is absent, the native input remains visible and
focused for manual selection. Request-specific result channels are unchanged.

`apps/wordcraft-web/examples/picker_check.rs` exercises the actual picker module.
Build it with `cargo build -p wordcraft-web --example picker_check --target
wasm32-unknown-unknown`, then run wasm-bindgen (matching Cargo.lock) with `--target
web` and serve its output. Browser automation can call the exported async
`pick_files(many)`, supplying files through the generated `input[type=file]`.
The exported `check_dismissal()` self-check verifies empty results and input cleanup
for change, cancel, pointerdown and keydown. Suppress native input clicks in automation
when deliberately testing silent dismissal.

Regression cases: normal change and cancel; Escape on a focused visible input;
suppress both events and return to the page with a pointerdown or keydown; suppress change with a selected FileList;
multiple files; no user activation; repeated requests; removed old inputs cannot
complete a new request. Check the file input and listeners are removed each time.

The full-PR Codex source review also identified an export completing before a failed
editable-document save incorrectly confirming that revision. Only lossless document
saves now advance confirmed save authority. A failing-then-passing regression covers
cancellation and failure while the owning tab is inactive and verifies direct close
still refuses its unsaved work. The same review identified the focused input's Escape
path, now covered by the browser harness.

CodeRabbit's summary concern about a drop selecting an existing tab was also
reproduced: drop cleanup cleared that tab's restored AutoSave authorization. File open
already owns save-authority initialization, so the redundant drop reset was removed.
Its regression preserves the existing tab's identity, dirty edits and AutoSave authority.

## Quality gate

After the latest Quit-review, cross-window Open, parked-view and path-copy fixes,
`cargo xtask ci` passed all eight stages. The workspace run included 1,066 UI tests
(two ignored), 1,071 engine tests, the translation audit and the 16-crate WASM check.
The detailed untranslated report remains unavailable without a local `CRAFT_I18N_DIR`
checkout.

`cargo xtask ci` passed all eight stages locally: formatting, workspace Clippy with
warnings denied, workspace tests, translation audit, doctests, asset attribution,
dependency layering and the 16-crate WASM check. The workspace test stage passed
4411 tests after follow-up remediation (4395 after upstream reconciliation; 4196 on
the original integration base). Existing ignored benchmarks remain ignored. A separate
`cargo check -p wordcraft-web --target wasm32-unknown-unknown` also passed, covering the
browser host code omitted from the standard 16-crate WASM gate. The actual Rust/WASM
picker module passed 29 headless Chromium runtime checks plus its four exported
dismissal self-checks, including selected FileLists,
normal change/cancel, Escape, suppressed-event dismissal through pointer and keyboard,
selection before a late change, multi-select, absent activation, stale inputs and 20
repeated cycles. Each completion removed the input and all four registered listeners.
The visible fallback input screenshot was inspected. Native OS picker dismissal was
simulated by suppressing change/cancel; manual OS-dialog interaction, Safari/Firefox
and Windows/Linux native behavior were not tested in this follow-up.
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

## PR #4359 inline findings

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

## Final PR review follow-up

The latest local reviews exposed seven behavior gaps. Addressed control-channel Save As now
uses the same duplicate-file guard as the active tab; Start's placeholder is excluded from
workspace persistence; recently closed files retain native `PathBuf`s; browser picker setup
failures reach the visible status line; Move Tab controls follow the pinned-group boundary;
Enter can search document text when Reopen Closed Tab is the only disabled tab match; and
addressed close commands reject non-object params without panicking. Regression coverage
includes those cases. The full workspace gate also exposed missing `Page` and `Rgb`
imports in the existing layout linearity test, now restored. Its proofing-text timing
sample was reduced from 50,000 to 1,000 runs to avoid a cache-size discontinuity; the
8× comparison and 24× failure threshold still distinguish the quadratic walk.
After those fixes, `cargo xtask ci` passed all eight gates: fmt, Clippy, workspace tests,
translation audit, doctests, assets, layers and WASM. Windows/Linux native behavior and a
browser end-to-end run remain unverified locally, as noted above.

The final Codex and CodeRabbit reviews found an Open picker that cancelled after its
origin closed, a skipped untitled name after final-tab close, and browser read errors
cleared by same-frame document changes. Open now falls back to the active window, final
close releases the stand-in name, and queued picker errors are posted after app logic.
Regression coverage includes all three. CodeRabbit also found deleted historical rows,
duplicate status/heading text and a mismatched finding count; the revision rows were restored
and the documentation corrected. An explicit Open picker is app-level, so its fallback does
not weaken cancellation for document-bound insertion/import pickers.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-26 | minor | Final review follow-up covers seven tab safety/search/picker findings and repairs missing layout-test imports required by the workspace gate. |
| 2026-10-26 | minor | Cross-window tab close, duplicate Open at the tab cap and serialized secondary-window saves; CodeRabbit cleanup. |
| 2026-10-26 | minor | Browser dismissal/Escape and save-authority review fixes; Chromium runtime harness and Codex review verified |
| 2026-10-26 | minor | Cover interrupted Quit, dictionary persistence, editing language and keyboard recording; require Codex review before pushing |
| 2026-10-26 | minor | PR review regressions cover secondary-window workspace ownership, Start disabled, modal scope, search and dirty callbacks |
| 2026-10-26 | major | Record tab safety regressions, process-termination recovery and screenshot inspection |
