# Diagnostics, DEC state reports, and Unicode baseline

Implementation checkpoint, 2026-10-09. This is not completion of the 22-group plan.
Both repositories remain dirty; no public release or main-client installation.

## C02 diagnostic completion in this batch

TL-DIAG/2 adds detection byte/code point, build revision and dirty status, width/output
profiles, and actual failed-closed versus rejected-operation impact. Private/ANSI mode
and SGR failures retain the rejected parameter index and original numeric spelling.
Incomplete previews never claim a complete parameter. OSC/DCS/APC payloads remain
redacted from both hex and text. JNI and .NET use V2 with missing-symbol fallback to V1.

Windows stores first-root evidence using DPAPI CurrentUser, per-session IDs, seven-day
and 256-file limits. Local UI details are read-only and copyable; generic record
serialization and ToString exclude evidence. Android distinguishes mirror failure
from loss of the Windows process or network. Headless tests verify overlay behavior
and evidence exclusion; actual main-product UI taps and authority-to-Android failure
delivery remain outstanding.

## C08 implemented protocol scope

| Operation | Behavior / bounds |
|---|---|
| CSI 2;1 $u, CSI 2;2 $u | DEC palette report, HLS or RGB percentages; only defined palette entries |
| DCS 2 $p … ST | Restore up to 256 unique palette entries, validate before mutation |
| CSI 1 $w | Cursor, page, defined DEC rendition/protection/origin/wrap flags, GL/GR/SS, G0–G3 designations |
| DCS 1 $t … ST | Restore the defined DEC subset, preserving unrelated rendition attributes |
| CSI 2 $w / DCS 2 $t … ST | Report visible tab stops; restore strictly increasing unique stops in 1..512 including hidden stops |

Restores collect at most 16 KiB; existing query payloads retain their 4 KiB limit.
Unknown read-only report selectors are ignored. Unsupported restore selectors, bad
fields, duplicates and truncated payloads do not partially mutate state. CAN/SUB
cancels collection. Existing Snapshot persists all restored state without a new
state section. Page number is currently 1; C09 must extend reports/restores and the
rectangle checksum to the final page store. DEC reports are intentionally a subset,
not a lossless replacement for TermLink Snapshot; RGB percentages can quantize colors.

Windows and Android strictly validate all three reply families. Windows suppresses
the corresponding outer-terminal reply by report family, with split and invalid-reply
tests. New JNI test restores tabs and palette byte by byte, installs a checkpoint,
then verifies replies and cursor movement. No restore replays a host action.

C14 OSC 21 window-title alias also uses the existing title state/stack, renderer and
ordered Windows title observer. Core split/Unicode/snapshot tests pass. The final
Unicode V4 five-library export includes this change and precise SGR component diagnostics.

Reference behavior: Microsoft Terminal revision
`6676c6f938f7e98ad680f0cecedd3bafb6529734` (`adaptDispatch.cpp`, DEC report methods).
TermLink implementation is original. Differences: bounded atomic validation rather
than partial field application; undefined colors are not fabricated; page extension
remains a tracked dependency.

## S0 performance baseline

Identical synthetic benchmark, release Rust 1.97.1, 5 warmups + 25 measured iterations.
Old source: core HEAD `e6e849172c6fca0de01ae0799eae97ad9483e687` archived into ignored
`target/capability-baseline-e6e8491`; current dirty source through C08/OSC21. These
measurements are not the final Unicode/graphics/UI/Relay performance gate.

| Case | Old median µs | Current median µs | Change |
|---|---:|---:|---:|
| 557,056 bytes text output | 350,860 | 354,681 | +1.09% |
| Snapshot export, 983,576 bytes | 39,361 | 40,316 | +2.43% |
| Snapshot import | 18,070 | 19,134 | +5.89% |
| 120 × 40 Render V3 export | 8,109 | 8,434 | +4.01% |

Raw samples and p95: application `artifacts/validation/s0-performance-{old,current}-20261009.jsonl`.
No measured median exceeds the plan's 10% regression threshold in this batch. These
are core wall times, not app frame times or network throughput.

## C03 Unicode and Render V4 implementation

Pinned Unicode 16.0.0 property data, original headers, License V3, source locations and
SHA-256 manifest are under `third_party/unicode/16.0.0`. Offline generator verifies all
hashes and deterministic generated output (`--check`). Original constant-memory UAX #29
segmenter passes **all 1,093 official vectors**. Explicit width profile 2 supports up
to 48 UTF-8 bytes per grapheme, Snapshot 2.12 and additive Render V4. Profile 1 remains
the default and preserves its 14-byte limit and frozen legacy snapshots. Mode 2027
controls joining in profile 2; profile 1 still reports it unsupported. Full profile
attachment negotiation is pending, so production sessions do not select profile 2 yet.

Config V4 is exactly 112 bytes; JNI registrations are optional for old Kotlin classes.
Render V1–V3 reject profile 2 with status 15 (V4 required), never reinterpret its bytes.
C# and Kotlin strictly decode V4, and Android draws full graphemes with copy text intact.
Every UTF-8 split, REP, mode changes, cross-SGR marks, cursor movement, wrap/resize,
history export, restore/suffix equivalence and 1,000 deterministic mixed operations
are covered. Maximum canonical V4 is 22,308,104 bytes (32 MiB bound); maximum canonical
Snapshot is 61,670,073 bytes (64 MiB bound). Unicode license ships with both clients.

After ASCII fast paths, 25 measured samples following 5 warmups give legacy/Unicode16
median µs: text 344,807/356,433; Snapshot export 34,548/34,559; import 12,604/13,161;
Render V3/V4 3,051/3,183. Each modern median is within 5% of current legacy, and within
the original baseline's 10% threshold. These are core timings, not UI/Relay latency.
Raw logs: `s2-unicode-performance-*-optimized-20261009.jsonl` in app validation artifacts.

## Verification checkpoint

- Rust C08 full suite: 506 core + 10 golden + 49 FFI passed; Clippy all targets passed.
- Subsequent OSC21 targeted tests: 6 passed; subsequent Unicode foundation: 3 passed.
- Windows native validation and SessionHost validation passed, including new report routing.
- Android API/native unit suites passed; API 36 `emulator-5556`: **28 JNI tests passed**.
- DPAPI/desktop headless suite passed, including generic-log/JSON evidence exclusion.
- Latest final core export: 519 core + 10 golden + 50 FFI tests and all-target Clippy passed.
- Final five-library lock verification, C# native validation, Android unit suites and
  **31 real JNI/View tests on emulator-5556 passed** after precise SGR token diagnostics.
  Logs: `s2-unicode-final-core-20261009.log`, `s2-unicode-final-native-export-20261009.log`,
  `s2-unicode-v4-final-{dotnet,android}-20261009.log`.
- Actual Unicode View screenshot reviewed: `s2-unicode16-render-v4-20261009.png`.
- Logs: app `artifacts/validation/s4-state-reports-*`, `s1-diagnostic-privacy-headless-*`,
  `s2-unicode16-conformance-*`. Do not equate headless/JNI with product Relay completion.

Remaining: C02 universal rejected-token/context coverage and actual UI/relay diagnostics;
new-profile attach negotiation; C01; C03 Windows renderer/product integration; C04; C09–C13; selection-color
C14; H01–H03; E02–E04; G01; native Windows rendering, resources, full product/old-version
matrix, ARM64 and long-soak gates. Existing C05/C06/C07/H04 still need product gates.
