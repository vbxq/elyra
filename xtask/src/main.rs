use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let command = env::args().nth(1).unwrap_or_else(|| "help".to_string());
    match command.as_str() {
        "ci" => ci(),
        "ci-full" => ci_full(),
        "help" | "-h" | "--help" => {
            println!("usage: cargo xtask <ci|ci-full>");
            Ok(())
        }
        other => Err(format!("unknown command '{other}'")),
    }
}

fn ci() -> Result<(), String> {
    cargo(&["fmt", "--all", "--", "--check"])?;
    cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
    ])?;
    cargo(&["test", "--workspace", "--all-features"])?;
    command("git", &["diff", "--check"], &[])
}

fn ci_full() -> Result<(), String> {
    ci()?;
    cargo(&["test", "--workspace", "--all-features", "--release"])?;

    optional_tool(
        "Miri",
        tool_available("cargo", &["+nightly", "miri", "--version"]),
        || {
            command(
                "cargo",
                &[
                    "+nightly",
                    "miri",
                    "test",
                    "-p",
                    "aelys-bytecode",
                    "-p",
                    "aelys-runtime",
                ],
                &[("MIRIFLAGS", "-Zmiri-disable-isolation")],
            )
        },
    )?;

    optional_tool(
        "AddressSanitizer",
        tool_available("cargo", &["+nightly", "--version"]),
        || {
            command(
                "cargo",
                &[
                    "+nightly",
                    "test",
                    "-p",
                    "aelys-runtime",
                    "--target",
                    "x86_64-unknown-linux-gnu",
                ],
                &[("RUSTFLAGS", "-Zsanitizer=address")],
            )
        },
    )?;

    optional_tool(
        "fuzz smoke",
        Path::new("fuzz/Cargo.toml").is_file()
            && tool_available("cargo", &["+nightly", "fuzz", "--version"]),
        || {
            command(
                "cargo",
                &[
                    "+nightly",
                    "fuzz",
                    "run",
                    "smoke",
                    "--",
                    "-max_total_time=10",
                ],
                &[],
            )
        },
    )?;

    cargo(&["bench", "--workspace", "--no-run"])
}

fn cargo(args: &[&str]) -> Result<(), String> {
    command("cargo", args, &[])
}

fn optional_tool<F>(name: &str, available: bool, run: F) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    if available {
        return run();
    }
    if env::var_os("AELYS_CI_FULL_STRICT").is_some() {
        return Err(format!("{name} is required in strict mode"));
    }
    eprintln!(
        "xtask: skipping {name}; install its local toolchain or set AELYS_CI_FULL_STRICT=1 to require it"
    );
    Ok(())
}

fn tool_available(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn command(program: &str, args: &[&str], envs: &[(&str, &str)]) -> Result<(), String> {
    let rendered = std::iter::once(OsString::from(program))
        .chain(args.iter().map(OsString::from))
        .map(|part| part.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    println!("xtask: {rendered}");

    let status = Command::new(program)
        .args(args)
        .envs(envs.iter().copied())
        .status()
        .map_err(|error| format!("failed to start '{rendered}': {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("'{rendered}' failed with {status}"))
    }
}
