use std::{env, fs, path::PathBuf};

/// Bundled resources that are build outputs in target/release (see tauri.conf.json).
const BUILT_RESOURCES: [&str; 2] = ["DirectML.dll", "clippro-cli.exe"];

fn main() {
    let profile_dir = env::var_os("OUT_DIR").map(PathBuf::from).and_then(|p| p.ancestors().nth(3).map(PathBuf::from));
    if let Some(dir) = &profile_dir {
        // ort-sys links DirectML.dll into target/<profile> as a symlink to its download cache when
        // Windows allows symlinks (CI runners, Developer Mode). tauri-build then copies the bundled
        // resource onto itself through the link and fails with a sharing violation, so make it a
        // real file first. ort-sys leaves a real file alone.
        let dll = dir.join("DirectML.dll");
        if dll.is_symlink() {
            if let Ok(real) = fs::canonicalize(&dll) {
                let _ = fs::remove_file(&dll);
                fs::copy(&real, &dll).expect("copy DirectML.dll over its symlink");
            }
        }
        // A plain cargo build (clippy, tests, a fresh checkout) may not have built them yet;
        // `tauri build` always has (beforeBuildCommand builds the CLI first). Leave the missing
        // ones out rather than fail, unless the Tauri CLI already passes a config of its own.
        let missing: Vec<_> = BUILT_RESOURCES.iter().filter(|f| !dir.join(f).exists()).collect();
        if !missing.is_empty() && env::var_os("TAURI_CONFIG").is_none() {
            let keys: Vec<_> = missing.iter().map(|f| format!("\"../../target/release/{f}\": null")).collect();
            env::set_var("TAURI_CONFIG", format!("{{\"bundle\": {{\"resources\": {{{}}}}}}}", keys.join(", ")));
            for f in &missing {
                println!("cargo:rerun-if-changed={}", dir.join(f).display());
            }
        }
    }
    tauri_build::build()
}
