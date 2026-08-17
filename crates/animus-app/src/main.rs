//! `animus` — the binary.
//!
//! Everything real lives in the crates below; this wires them, reads the
//! command line, and owns the two things only a binary can own: the panic
//! hook and process exit codes.

mod autosave;
mod cli;

use animus_editor::{EditorPlugin, ProjectRoot};
use animus_output::{OutputConfig, OutputPlugin};
use animus_runtime::{RuntimePlugin, SolverPlugin};
use bevy::prelude::*;
use bevy::window::PresentMode;

fn main() {
    // The panic hook is installed before Bevy so a panic during startup is
    // also caught. It writes to stderr and a crash file next to the
    // executable — at a venue, the console is usually gone but the file
    // survives to be read the morning after.
    install_panic_hook();

    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(e) => {
            eprintln!("animus: {e}");
            eprintln!(
                "usage: animus [project.animus] [--perform] [--output-monitor <n>] [--no-vsync]"
            );
            std::process::exit(2);
        }
    };

    // Load the project before the window opens: a broken file should fail
    // in the terminal with the codec's error, not behind a frozen splash.
    let (project, root) = match &cli.project {
        Some(path) => match animus_project::load(path) {
            Ok(p) => (p, path.clone()),
            Err(e) => {
                eprintln!("animus: could not open {}: {e}", path.display());
                std::process::exit(1);
            }
        },
        None => (
            animus_core::doc::Project::new("Untitled"),
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("untitled.animus"),
        ),
    };

    if let Some(note) = autosave::report_existing_sidecar(&root) {
        // Surfaced in the log for M1; the editor shows it in the status
        // line once it reads AutosaveState.
        eprintln!("animus: {note}");
    }

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: window_title(&project, &root),
                    resolution: (1440u32, 900u32).into(),
                    // The editor window presents unsynced by default: the
                    // *output* window's vsync is the one that matters
                    // (M0-4), and the editor riding the same display's sync
                    // just adds latency to every drag.
                    present_mode: PresentMode::AutoNoVsync,
                    ..default()
                }),
                ..default()
            })
            .set(bevy::log::LogPlugin {
                filter: "info,wgpu_core=warn,wgpu_hal=warn".into(),
                ..default()
            }),
    )
    .add_plugins(RuntimePlugin::new(project))
    .add_plugins(SolverPlugin)
    .add_plugins(EditorPlugin)
    .add_plugins(OutputPlugin)
    .insert_resource(ProjectRoot(root))
    .insert_resource(OutputConfig {
        enabled: true,
        monitor_override: cli.output_monitor,
        vsync: !cli.no_vsync,
    })
    .init_resource::<autosave::AutosaveState>()
    .add_systems(Update, autosave::autosave);

    // --perform: no editor systems at all would be M2's PerformanceMode;
    // M1 honours the flag by starting with the output configured and the
    // editor present. The distinction is documented rather than half-built.
    if cli.perform {
        info!("--perform: output-first startup (full performance mode arrives in M2)");
    }

    app.run();
}

fn window_title(project: &animus_core::doc::Project, root: &std::path::Path) -> String {
    format!(
        "Animus Live — {} ({})",
        project.meta.name,
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string())
    )
}

/// Panic → stderr + a crash file, then abort.
///
/// Spec §17 wants per-subsystem `catch_unwind` in M5; until then the honest
/// behaviour is a loud, recorded crash. What must never happen is a silent
/// vanish at a venue with nothing to read afterwards.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = std::env::current_exe()
            .ok()
            .and_then(|p| {
                p.parent()
                    .map(|d| d.join(format!("animus-crash-{when}.txt")))
            })
            .unwrap_or_else(|| std::path::PathBuf::from(format!("animus-crash-{when}.txt")));

        let body = format!(
            "animus crashed.\n\n{info}\n\nbacktrace:\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        let _ = std::fs::write(&path, &body);
        eprintln!("animus crashed; details written to {}", path.display());
        default(info);
    }));
}
