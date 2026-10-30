//! ONNX Runtime sessions: DirectML on the GPU when available, else the CPU; CPU only in
//! deterministic mode (SPEC §18).

use ort::environment::Environment;
use ort::session::Session;
use std::path::Path;
use std::sync::OnceLock;

static ENV: OnceLock<Result<Environment, String>> = OnceLock::new();

fn env() -> Result<&'static Environment, String> {
    ENV.get_or_init(|| ort::init().with_name("clippro").build().map_err(|e| e.to_string())).as_ref().map_err(Clone::clone)
}

/// Threads for one CPU session (AI never takes the whole machine from core analysis).
fn cpu_threads() -> usize {
    std::thread::available_parallelism().map(|n| (n.get() / 2).max(1)).unwrap_or(2)
}

/// Opens a model. `deterministic` keeps it on the CPU with one execution order.
pub fn session(model: &Path, deterministic: bool) -> Result<Session, String> {
    let builder = Session::builder(env()?).map_err(|e| e.to_string())?;
    let builder = builder.with_intra_threads(cpu_threads()).map_err(|e| e.to_string())?;
    let mut builder = if deterministic {
        builder.with_deterministic_compute(true).map_err(|e| e.to_string())?
    } else {
        builder
            .with_execution_providers([ort::ep::DirectML::default().build(), ort::ep::CPU::default().build()])
            .map_err(|e| e.to_string())?
    };
    builder.commit_from_file(model).map_err(|e| format!("{}: {e}", model.display()))
}
