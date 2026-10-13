# DEC line renditions and dual-platform frame contracts

Development checkpoint, 2026-10-09. This records C04 core/binding/Android View work;
the 22-group plan and Windows native display/Relay acceptance remain open.

## Protocol and state

Original implementation of ESC #3/#4 (double-height upper/lower half), #5 (single
width), #6 (double width), #8 (alignment pattern), based on the documented DEC
behavior and Microsoft Terminal `6676c6f938f7e98ad680f0cecedd3bafb6529734`,
`adaptDispatch.cpp` SetLineRendition/EraseInDisplay/ScreenAlignmentPattern.
No third-party implementation code was copied.

Each stored row has a bounded four-state rendition. Scaled rows use half the
physical columns (floor, minimum one), crop inaccessible cells including split
wide glyphs, and remain fixed rows on resize. Ordinary logical lines continue to
reflow. Setting a line rendition breaks soft-wrap joins on both sides; scroll,
history retention, main/alternate buffers and immutable renderer windows carry
the rendition with the row. Cursor and saved cursor are clamped to effective
columns; pending wrap is revalidated when scrolling changes the row beneath them.

EL retains line rendition. ED resets only fully erased rows; alignment fills
visible cells with default-rendition E, resets origin/margins and homes the cursor.
DECSLRM clears visible line scaling, and line-rendition commands are ignored while
left/right margins are enabled, matching the reference behavior.

Snapshot 2.13 requires feature `0x4000000`. Rows encode the rendition in flag bits
2–3; older snapshots retain their canonical bytes and flags. Unicode width remains
independently explicit. Import rejects unknown flags, scaled soft wraps, incoming
soft wraps into a scaled row, impossible meaningful ends/cursors and old-reader
feature combinations. No row/image/pixel resource allocation is introduced.

The unpublished additive Render V4 contract at this checkpoint required `0x1ff` and carries the
rendition in row flag bits 1–2. C#/Kotlin validate the effective row and hidden
cells. V1–V3 remain frozen and explicitly reject scaled rows instead of silently
displaying a different layout. Config V4 and C/JNI function signatures are unchanged.

## Android presentation

The existing NativeTerminalView scales each row's text and background, clips the
upper/lower half of double-height text, and applies horizontal scaling to cursor,
cursor-follow, selection handles and hit testing. Copy excludes inaccessible
cells. CLI page layout and ordinary single-width interactions remain unchanged.

## Evidence

- 524 core + 10 golden + 50 FFI tests passed; all-target Clippy passed.
- New tests cover every input split, both width profiles, odd columns, wide-cell
  clipping, margins, erase, resize/reflow, history, main/alternate restore and
  malformed snapshots. A deterministic 1,000-iteration edit/scroll/resize/restore
  test caught and now protects the stale pending-wrap bug.
- Five DEV libraries exported and verified against the lock. Final Windows
  NativeTerminalCoreValidation passed with real V4 line-rendition round trips.
- Android API/native units and 32 JNI/View tests on API 36 emulator-5556 passed.
  Bitmap test checks actual double-width ink and both double-height halves.
  Reviewed `s2-scaled-lines-v4-20261009.png` shows normal/scaled WWWW and joined
  upper/lower ABCD. Antialiased background edges are excluded by contrast threshold.
- Logs under app `artifacts/validation/`: `s2-lines-final-core-20261009.log`,
  `s2-lines-final-native-export-20261009.log`, `s2-lines-final-dotnet-20261009.log`,
  `s2-lines-final-android-20261009.log`.
- Isolated release benchmark (5 warmups, 25 samples): median µs text 348,558,
  Snapshot export 34,201, import 13,351, Render V3 3,210. These remain within the
  original baseline's 10% threshold. `s2-lines-performance-isolated-20261009.jsonl`
  is the comparison run; the earlier run overlapped platform builds and is not
  used as the performance gate. This does not establish UI/Relay performance.

Self-review only. Both repositories remain dirty; no commit, public release,
main-client replacement or server deployment. Profile attachment negotiation,
Windows core-frame presentation, actual Relay/mixed-version and ARM64 product
verification are still required before calling C04 product-complete.

Subsequent color/mouse V4 contract (`0x7ff`) and live Windows display/fit evidence:
[S2 colors and native display](S2_COLORS_NATIVE_DISPLAY_20261009.md).
