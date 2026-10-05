# Terminal Core repository

- This is the separately maintained proprietary TermLink terminal core. Preserve the current license declaration; do not grant a new public license.
- The application repository is the sibling `../termlink`. Do not copy application source, credentials, binaries, or its Git history into this repository.
- Keep terminal state, C ABI, Windows P/Invoke, and Android JNI contracts compatible. Splitting repositories must not change terminal behavior.
- Run pinned Rust 1.97.1 tests from this repository. Its `.cargo/config.toml` preserves the Windows static CRT setting.
- Local golden stream fixtures are repository-owned capture inputs copied from the application repository; preserve their provenance.
- Do not upload source until the remote is verified private and pushing is authorized.
- Builds on this workstation use D:\Dev inventory and Ensure-DevEnv.ps1. Do not change system PATH or services automatically.
