mod cli;

// every compiler pass recurses once per nesting level, and debug frames reach several KiB each
const STACK_BYTES: usize = 256 * 1024 * 1024;

fn main() {
    let code = std::thread::Builder::new()
        .name("main".to_string())
        .stack_size(STACK_BYTES)
        .spawn(cli::run)
        .map(|worker| {
            worker
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
        .unwrap_or_else(|_| cli::run());
    if code != 0 {
        std::process::exit(code);
    }
}
