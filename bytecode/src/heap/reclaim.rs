use crate::object::{GcObject, ObjectKind};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};

type Garbage = Box<dyn Send>;

static RECLAIMER: OnceLock<Option<Sender<Garbage>>> = OnceLock::new();

// started with the first heap so that its malloc arena is a fresh one, not an arena a finished thread left for reuse
pub(super) fn start() {
    reclaimer();
}

fn reclaimer() -> Option<&'static Sender<Garbage>> {
    RECLAIMER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel::<Garbage>();
            std::thread::Builder::new()
                .name("aelys-gc-free".to_string())
                .spawn(move || {
                    std::hint::black_box(Box::new(0u8));
                    for garbage in receiver {
                        drop(garbage);
                    }
                })
                .ok()
                .map(|_| sender)
        })
        .as_ref()
}

fn storage(kind: ObjectKind) -> Option<Garbage> {
    match kind {
        ObjectKind::Vec(vector) => Some(Box::new(vector)),
        ObjectKind::Array(array) => Some(Box::new(array)),
        ObjectKind::Map(map) => Some(map),
        ObjectKind::Function(function) => Some(function),
        ObjectKind::String(mut string) => string
            .take_unique_bytes()
            .map(|bytes| Box::new(bytes) as Garbage),
        _ => None,
    }
}

// returning a large buffer to the os unmaps its pages, which costs about 0.15 ms per MiB on the collecting thread
pub(super) fn release(object: GcObject) {
    let Some(garbage) = storage(object.kind) else {
        return;
    };
    match reclaimer() {
        Some(sender) => {
            if let Err(error) = sender.send(garbage) {
                drop(error.0);
            }
        }
        None => drop(garbage),
    }
}
