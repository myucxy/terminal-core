# Golden replay input provenance

These ten `output.bin` files are unmodified TermLink-owned ConPTY captures copied
from `myucxy/TermLink@78c011b`, `testdata/terminal-golden-streams/<case>/output.bin`,
when the terminal core became a separate repository on 2026-10-05.

`SHA256.json` records each original content hash. The application keeps its full
capture metadata and expectations; the core only needs the output bytes used by
`crates/termlink-terminal-core/tests/golden_streams.rs`.

They are test inputs, not third-party program source. Names such as vim, less,
and htop identify the recorded session. Keep this provenance and update hashes
explicitly if a future capture changes.
