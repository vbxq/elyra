use std::path::{Path, PathBuf};
use std::process::Command;

const CHILD_SCRIPT: &str = "T4_SDL_CHILD_SCRIPT";
const FRAMES: i64 = 24;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn sdl_available() -> bool {
    Command::new("pkg-config")
        .args(["--exists", "sdl2"])
        .status()
        .is_ok_and(|status| status.success())
}

fn build_sdl_module() -> PathBuf {
    let crate_dir = repo_root().join("modules/sdl");
    let lock = crate_dir.join("Cargo.lock");
    if !lock.exists() {
        std::fs::copy(repo_root().join("Cargo.lock"), &lock).unwrap();
    }
    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/t4_sdl");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "--manifest-path"])
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .status()
        .expect("cargo build runs");
    assert!(status.success(), "the sdl module must build");
    let library = target_dir.join("release/libaelys_sdl.so");
    assert!(library.exists(), "missing {}", library.display());
    library
}

fn run_child(script: &Path) {
    let value = aelys_driver::run_file_with_config(
        script,
        aelys_runtime::VmConfig::default(),
        vec![FRAMES.to_string()],
    )
    .unwrap_or_else(|error| panic!("the example failed: {error}"));
    assert!(value.is_unit(), "{value:?}");
}

#[cfg(target_os = "linux")]
#[test]
fn the_gradient_example_presents_frames_headless() {
    if let Some(script) = std::env::var_os(CHILD_SCRIPT) {
        run_child(Path::new(&script));
        return;
    }
    if !sdl_available() {
        eprintln!("skipped: SDL2 development files are not installed");
        return;
    }
    let library = build_sdl_module();
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(&library, dir.path().join("sdl.so")).unwrap();
    let script = dir.path().join("gradient.aelys");
    std::fs::copy(repo_root().join("examples/sdl/gradient.aelys"), &script).unwrap();

    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "the_gradient_example_presents_frames_headless",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_SCRIPT, &script)
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("the child test runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");

    let frames_line = stdout
        .lines()
        .find_map(|line| line.find("frames=").map(|at| &line[at..]))
        .unwrap_or_else(|| panic!("no frames line in\n{stdout}\n{stderr}"));
    assert!(
        frames_line.starts_with(&format!("frames={FRAMES} quit=false ")),
        "{frames_line}"
    );
    let elapsed_ms: i64 = frames_line
        .rsplit("elapsed_ms=")
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let paced_ms = (FRAMES - 1) * 1000 / 60;
    assert!(
        elapsed_ms >= paced_ms && elapsed_ms < paced_ms + 5000,
        "{FRAMES} frames paced at 60 Hz take about {paced_ms} ms, got {elapsed_ms}"
    );

    let last = FRAMES - 1;
    let expected = ((20 + last) % 256) * 65536 + 20 * 256 + (255 - 20);
    assert!(
        stdout.contains(&format!(
            "indexed_pixel={expected} indexed_ok=true bottom_ok=true argb_ok=true"
        )),
        "{stdout}"
    );
}
