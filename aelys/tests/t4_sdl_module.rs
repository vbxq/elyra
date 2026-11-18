use std::path::{Path, PathBuf};
use std::process::Command;

const CHILD_SCRIPT: &str = "T4_SDL_CHILD_SCRIPT";
const SKIP_WITHOUT_SDL: &str = "AELYS_SKIP_SDL_TESTS";
const FRAMES: i64 = 24;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn require_sdl() -> bool {
    let found = Command::new("pkg-config")
        .args(["--exists", "sdl2"])
        .status()
        .is_ok_and(|status| status.success());
    if found {
        return true;
    }
    assert!(
        std::env::var_os(SKIP_WITHOUT_SDL).is_some(),
        "pkg-config finds no sdl2; install the SDL2 development files or set {SKIP_WITHOUT_SDL}=1 to skip"
    );
    eprintln!("skipped: {SKIP_WITHOUT_SDL} is set and SDL2 is not installed");
    false
}

fn build_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "deps"))
        .and_then(Path::parent)
        .expect("the test binary lives in a cargo deps directory")
        .join("t4_native_crates")
}

fn build_sdl_module() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT.get_or_init(build_sdl_module_once).clone()
}

fn build_sdl_module_once() -> PathBuf {
    let source = repo_root().join("modules/sdl");
    let root = build_dir();
    let crate_dir = root.join("aelys-sdl");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    for entry in std::fs::read_dir(source.join("src")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), crate_dir.join("src").join(entry.file_name())).unwrap();
    }
    let manifest = std::fs::read_to_string(source.join("Cargo.toml")).unwrap();
    let native = repo_root().join("native");
    let manifest = manifest.replace(
        "path = \"../../native\"",
        &format!("path = \"{}\"", native.display()),
    );
    assert!(
        manifest.contains(&native.display().to_string()),
        "{manifest}"
    );
    std::fs::write(crate_dir.join("Cargo.toml"), manifest).unwrap();
    let lock = repo_root().join("Cargo.lock");
    let mut command = Command::new(env!("CARGO"));
    command.args(["build", "--release", "--quiet"]);
    if lock.exists() {
        std::fs::copy(&lock, crate_dir.join("Cargo.lock")).unwrap();
        command.arg("--offline");
    }
    let status = command
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .status()
        .expect("cargo build runs");
    assert!(status.success(), "the sdl module must build");
    let library = root.join("target/release/libaelys_sdl.so");
    assert!(library.exists(), "missing {}", library.display());
    library
}

fn run_child(script: &Path) {
    let value = aelys_driver::run_file_with_config(
        script,
        aelys_runtime::VmConfig::default(),
        vec![FRAMES.to_string()],
    )
    .unwrap_or_else(|error| panic!("the script failed: {error}"));
    assert!(value.is_unit(), "{value:?}");
}

fn run_headless(test_name: &str, script: &Path) -> String {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(CHILD_SCRIPT, script)
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("the child test runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    stdout
}

fn prepare(script_name: &str, body: &str) -> (tempfile::TempDir, PathBuf) {
    let library = build_sdl_module();
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(&library, dir.path().join("sdl.so")).unwrap();
    let script = dir.path().join(script_name);
    std::fs::write(&script, body).unwrap();
    (dir, script)
}

#[cfg(target_os = "linux")]
#[test]
fn the_gradient_example_presents_frames_headless() {
    if let Some(script) = std::env::var_os(CHILD_SCRIPT) {
        run_child(Path::new(&script));
        return;
    }
    if !require_sdl() {
        return;
    }
    let example = std::fs::read_to_string(repo_root().join("examples/sdl/gradient.aelys")).unwrap();
    let (_dir, script) = prepare("gradient.aelys", &example);
    let stdout = run_headless("the_gradient_example_presents_frames_headless", &script);

    let frames_line = stdout
        .lines()
        .find_map(|line| line.find("frames=").map(|at| &line[at..]))
        .unwrap_or_else(|| panic!("no frames line in\n{stdout}"));
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

const SIZE_CHECKS: &str = r#"use std::bytes
use sdl
match sdl::open("sizes", 16, 48, 1) { Ok(_) => (), Err(e) => println("open failed {e}") }
let px = bytes::alloc(768)
let mut i = 0
while i < 768 {
    bytes::write_u8(px, i, i % 256)
    i += 1
}
let same = match sdl::present_indexed(px, px) { Ok(_) => "ok", Err(e) => "err" }
let pixel = match sdl::read_pixel(5, 0) { Ok(p) => p, Err(e) => -1 }
let long_px = match sdl::present_indexed(bytes::alloc(769), px) { Ok(_) => "ok", Err(e) => "err" }
let short_px = match sdl::present_indexed(bytes::alloc(767), px) { Ok(_) => "ok", Err(e) => "err" }
let long_pal = match sdl::present_indexed(px, bytes::alloc(769)) { Ok(_) => "ok", Err(e) => "err" }
let long_argb = match sdl::present_argb(bytes::alloc(3076)) { Ok(_) => "ok", Err(e) => "err" }
let short_argb = match sdl::present_argb(bytes::alloc(3068)) { Ok(_) => "ok", Err(e) => "err" }
let exact_argb = match sdl::present_argb(bytes::alloc(3072)) { Ok(_) => "ok", Err(e) => "err" }
println("same={same} pixel={pixel} long_px={long_px} short_px={short_px} long_pal={long_pal} long_argb={long_argb} short_argb={short_argb} exact_argb={exact_argb}")
sdl::close()
"#;

#[cfg(target_os = "linux")]
#[test]
fn the_sdl_module_takes_one_buffer_twice_and_rejects_wrong_frame_sizes() {
    if let Some(script) = std::env::var_os(CHILD_SCRIPT) {
        run_child(Path::new(&script));
        return;
    }
    if !require_sdl() {
        return;
    }
    let (_dir, script) = prepare("sizes.aelys", SIZE_CHECKS);
    let stdout = run_headless(
        "the_sdl_module_takes_one_buffer_twice_and_rejects_wrong_frame_sizes",
        &script,
    );
    let expected_pixel = 15 * 65536 + 16 * 256 + 17;
    assert!(
        stdout.contains(&format!(
            "same=ok pixel={expected_pixel} long_px=err short_px=err long_pal=err long_argb=err short_argb=err exact_argb=ok"
        )),
        "{stdout}"
    );
}
