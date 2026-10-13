# Extended colors and Windows native display checkpoint

Development only, 2026-10-09; both repositories retain their dirty state. This
checkpoint does not close the 22-group implementation plan or the product gates.

## C14 state and presentation

OSC 17 sets/queries the selection background; OSC 117 and exact `default` reset
it. CSI Pi;Pf;Pb , | assigns live color indexes for normal text (item 1) and
window frame (item 2). Indexes 0–255 address the palette; 256/257 are independent
default foreground/background slots. Defaults reference those slots rather than
copying RGB values. OSC 10/11 query/set/reset follows the active alias, so later
palette changes remain visible. DECRQSS `,|`, `1,|`, `2,|` reports the assignments.

This is original code based on documented behavior and Microsoft Terminal
`6676c6f938f7e98ad680f0cecedd3bafb6529734` AssignColor / RequestXtermColorResource.
No implementation was copied. Unknown items and out-of-range indexes fail with
the exact original parameter spelling in TL-DIAG/2; invalid batches do not commit.

Extended color state selects Snapshot 2.14, required feature `0x8000000`, with
OSC codec 3. Empty extensions retain the prior canonical snapshot version. The
new selection value and four color references are bounded by 12 bytes. Maximum
canonical snapshot is 61,670,085 bytes, below the unchanged 64 MiB limit.

The unpublished Render V4 now requires `0x7ff`: frame flags include mouse routing
and alternate scroll, palette section has 264 records (259 legacy values,
selection RGB, four color indexes). Maximum canonical V4 is 22,308,128 bytes,
below 32 MiB. Old render entry points explicitly reject extended presentation
state. C#/Kotlin validate all new values and reject prior draft V4 features.

Android and Windows render actual selection/default/frame colors. Window-frame
colors affect the terminal's thin client border, not unrelated navigation or OS
chrome. Existing title behavior remains. Immutable historical line capture still
needs the planned E02 work to preserve all presentation attributes.

The outer Windows reply filter uses committed effects to determine color-query
ownership, including aliases. Multiple same-index queries inside one OSC 4 retain
the earlier external reply before a later set makes the color known. Queries
without an authoritative baseline still use the established outer fallback.

## Windows display and input

The opt-in SessionHost `--native-display` path opens an Avalonia terminal window
from the existing Desktop executable. It consumes immutable frames from the same
authority; no second terminal parser or shell. A separate current-user pipe uses
mutual PID checks, strict length budgets, a bounded input queue and deadlines.
Legacy Host/Agent IPC and the default outer-terminal launch path remain frozen.

The View renders full clusters, scaled lines, styles, selection and cursor;
existing CaskaydiaCove font and OFL notice are reused. Full glyph shaping reaches
Skia, but available Windows fallback glyphs still limit flag/emoji fidelity.
Text/IME, clipboard, focus and functional keys use the authority input encoder.
Win32 Enter includes scan code and CR; press/release pairs are written atomically.
Real live-window testing caught the missing CR field that unit-only tests missed.

Mouse tracking and alternate scroll come from V4 committed mode flags. Ordinary
wheel uses history; tracking uses core mouse encoding; alternate-scroll uses the
same encoder; Shift forces local selection/history. Historical frames never send
mouse input into the live app. Fractional wheel deltas accumulate. Idle historical
views suppress repeated full frames and changing offset forces a fresh frame.

Native phone fit and desktop restore use the existing authorized resize request.
The UI reports measured rows/columns before the host resizes ConPTY. Input epoch,
expiration, actual size, screen constraints and one-use restore point are checked.
Rejected/stale requests restore the prior window and do not close the session.

## Evidence

Application `artifacts/validation/` contains:

- `s2-colors-core-20261009.log`: 527 core + 10 golden + 50 FFI passed; subsequent
  exact DECAC token test passed in `s2-colors-targeted-20261009.log` (528 core total).
- `s2-colors-clippy-20261009.log`: all-target Clippy passed.
- `s2-colors-final-export-20261009.log`: five DEV libraries exported, hashes verified.
- `s2-colors-dotnet-20261009.log`: Windows native binding validation passed.
- `s2-colors-surface-20261009.log`: actual Skia pixels for selection/background,
  cluster copy, scaled rows, mouse/alternate/history routing passed.
- `s2-colors-sessionhost-20261009.log`: full SessionHost suite, color reply splits,
  nested ConPTY input and legacy outer fit/restore passed.
- `s2-colors-android-20261009.log`: API/native units and 33 JNI/View tests on
  emulator-5556 API 36 passed, including selection/background bitmap pixels.
- `s0-native-window-fit-20261009.log`: actual Avalonia window + real child/ConPTY,
  text/Enter echo, fit/restore, input-priority, expiry and impossible-size rejection.

Self-review only. Remaining Windows work includes profile negotiation/default
launch integration, full key release/repeat/Alt/keypad fidelity, queue-pressure
gesture handling, IME/accessibility, history anchors, theme and graphics resources.
Full product Relay/mixed-version, ARM64 and soak gates remain open. No Git push,
formal release, primary-client installation or server deployment occurred.
