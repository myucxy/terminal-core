# Terminal Core repository

- This is the separately maintained TermLink terminal core. As directed by the maintainer on 2026-10-09, project-owned code uses GPL-3.0-only; keep LICENSE, NOTICE and Cargo metadata consistent and preserve third-party terms.
- The application repository is the sibling `../termlink`. Do not copy application source, credentials, binaries, or its Git history into this repository.
- Keep terminal state, C ABI, Windows P/Invoke, and Android JNI contracts compatible. Splitting repositories must not change terminal behavior.
- Native Rust/C ABI implementation, Android JNI C++, CMake and DLL/SO build scripts belong here. `scripts/Build-ToTermLink.ps1` builds and exports the five pinned libraries to the application; do not put core compilation logic back in the application repository.
- Application-facing C#/Kotlin bindings and UI remain in the application. Changes to JNI registered names or C ABI require coordinated adapter validation.
- Run pinned Rust 1.97.1 tests from this repository. Its `.cargo/config.toml` preserves the Windows static CRT setting.
- Local golden stream fixtures are repository-owned capture inputs copied from the application repository; preserve their provenance.
- Public source release is planned separately. Do not change repository visibility or push without authorization. Before distributing binaries, provide matching Corresponding Source and build materials under GPLv3; a private repository URL is insufficient.
- Builds on this workstation use D:\Dev inventory and Ensure-DevEnv.ps1. Do not change system PATH or services automatically.
