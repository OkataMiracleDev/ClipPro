// Release builds are a GUI app (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `ClipPro --export-bindings <file>` writes the TypeScript bindings and exits. The Tauri
    // test harness can't load on Windows (no Common-Controls manifest), so the drift check
    // runs through the real binary (scripts/check-bindings.sh).
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--export-bindings") {
        let path = args.get(i + 1).cloned().unwrap_or_else(|| clippro_app::BINDINGS_PATH.to_string());
        match clippro_app::export_bindings(&path) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
    // `ClipPro --bench-preview <out.json> <clip>...` measures the preview transports inside
    // WebView2 with the window hidden, writes the report and exits (ADR-002).
    let bench = args.iter().position(|a| a == "--bench-preview").map(|i| clippro_app::Bench {
        out: args.get(i + 1).map(Into::into).unwrap_or_else(|| "bench-preview.json".into()),
        clips: args[(i + 2).min(args.len())..].iter().map(Into::into).collect(),
    });
    clippro_app::run(bench);
}
