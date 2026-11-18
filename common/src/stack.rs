use std::cell::Cell;

// every compiler phase recurses once per nesting level and debug frames reach several KiB each;
// the reservation is virtual, only the pages a deep program touches are committed
const DEEP_STACK_BYTES: usize = 256 * 1024 * 1024;

thread_local! {
    static ON_DEEP_STACK: Cell<bool> = const { Cell::new(false) };
}

pub fn with_deep_stack<T: Send>(task: impl FnOnce() -> T + Send) -> T {
    if ON_DEEP_STACK.get() {
        return task();
    }
    let mut task = Some(task);
    let outcome = std::thread::scope(|scope| {
        let slot = &mut task;
        std::thread::Builder::new()
            .name("aelys-compile".to_string())
            .stack_size(DEEP_STACK_BYTES)
            .spawn_scoped(scope, move || {
                ON_DEEP_STACK.set(true);
                slot.take().map(|task| task())
            })
            .ok()
            .map(|worker| worker.join())
    });
    match outcome {
        Some(Ok(Some(value))) => value,
        Some(Err(panic)) => std::panic::resume_unwind(panic),
        _ => (task
            .take()
            .expect("a task that never ran is still in its slot"))(),
    }
}

pub fn mark_deep_stack() {
    ON_DEEP_STACK.set(true);
}
