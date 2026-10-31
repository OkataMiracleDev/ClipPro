use std::{env, fs, path::PathBuf};

fn main() {
    // ort-sys links DirectML.dll into target/<profile> as a symlink to its download cache when
    // Windows allows symlinks (CI runners, Developer Mode). tauri-build then copies the bundled
    // resource onto itself through the link and fails with a sharing violation, so make it a
    // real file first. ort-sys leaves a real file alone.
    if let Some(dir) = env::var_os("OUT_DIR").map(PathBuf::from).and_then(|p| p.ancestors().nth(3).map(PathBuf::from)) {
        let dll = dir.join("DirectML.dll");
        if dll.is_symlink() {
            if let Ok(real) = fs::canonicalize(&dll) {
                let _ = fs::remove_file(&dll);
                fs::copy(&real, &dll).expect("copy DirectML.dll over its symlink");
            }
        }
    }
    tauri_build::build()
}
