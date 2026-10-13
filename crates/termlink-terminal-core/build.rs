use std::{env, path::Path, process::Command};

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("Cargo manifest directory");
    let root = Path::new(&manifest).join("../..");
    let git = |args: &[&str]| -> Option<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let revision = git(&["rev-parse", "HEAD"])
        .filter(|value| value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unavailable".into());
    let dirty =
        git(&["status", "--porcelain", "--untracked-files=normal"]).map_or("unknown", |status| {
            if status.is_empty() { "false" } else { "true" }
        });
    println!("cargo:rustc-env=TERMLINK_CORE_REVISION={revision}");
    println!("cargo:rustc-env=TERMLINK_CORE_SOURCE_DIRTY={dirty}");
    for path in [
        "src",
        "build.rs",
        "Cargo.toml",
        "../../Cargo.toml",
        "../../Cargo.lock",
        "../../.git/HEAD",
        "../../.git/index",
        "../../.git/refs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
