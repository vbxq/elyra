use libloading::Library;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub(crate) struct Opened {
    pub(crate) library: &'static Library,
    pub(crate) path: String,
}

pub(crate) struct NotFound {
    pub(crate) tried: Vec<String>,
    pub(crate) reason: String,
}

// a loaded library is never unloaded: C code keeps pointers into it (atexit handlers, callbacks, cached symbols)
fn loaded() -> &'static Mutex<HashMap<String, &'static Library>> {
    static LOADED: OnceLock<Mutex<HashMap<String, &'static Library>>> = OnceLock::new();
    LOADED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn is_explicit(name: &str) -> bool {
    name.contains('/')
        || name.contains('\\')
        || name.ends_with(".so")
        || name.contains(".so.")
        || name.ends_with(".dylib")
        || name.ends_with(".dll")
}

#[cfg(all(unix, not(target_vendor = "apple")))]
fn local_names(name: &str) -> Vec<String> {
    vec![format!("lib{name}.so"), format!("{name}.so")]
}

#[cfg(target_vendor = "apple")]
fn local_names(name: &str) -> Vec<String> {
    vec![format!("lib{name}.dylib"), format!("{name}.dylib")]
}

#[cfg(windows)]
fn local_names(name: &str) -> Vec<String> {
    vec![format!("{name}.dll"), format!("lib{name}.dll")]
}

#[cfg(all(unix, not(target_vendor = "apple")))]
fn system_candidates(name: &str) -> Vec<String> {
    let mut candidates = vec![format!("lib{name}.so")];
    let mut dirs: Vec<PathBuf> = std::env::var_os("LD_LIBRARY_PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default();
    let triple = format!("{}-linux-gnu", std::env::consts::ARCH);
    for dir in [
        "/lib64".to_string(),
        "/usr/lib64".to_string(),
        "/lib".to_string(),
        "/usr/lib".to_string(),
        "/usr/local/lib".to_string(),
        format!("/lib/{triple}"),
        format!("/usr/lib/{triple}"),
    ] {
        dirs.push(PathBuf::from(dir));
    }
    let prefix = format!("lib{name}.so.");
    let mut versioned: Vec<(Vec<u64>, String)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let file = entry.file_name();
            let Some(file) = file.to_str() else {
                continue;
            };
            let Some(version) = file.strip_prefix(&prefix) else {
                continue;
            };
            let Some(parts) = version
                .split('.')
                .map(|part| part.parse::<u64>().ok())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            versioned.push((parts, dir.join(file).display().to_string()));
        }
    }
    versioned.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    for (_, path) in versioned {
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    }
    candidates
}

#[cfg(target_vendor = "apple")]
fn system_candidates(name: &str) -> Vec<String> {
    vec![
        format!("lib{name}.dylib"),
        format!("/usr/local/lib/lib{name}.dylib"),
        format!("/opt/homebrew/lib/lib{name}.dylib"),
        format!("/System/Library/Frameworks/{name}.framework/{name}"),
        format!("/Library/Frameworks/{name}.framework/{name}"),
    ]
}

#[cfg(windows)]
fn system_candidates(name: &str) -> Vec<String> {
    vec![format!("{name}.dll"), format!("lib{name}.dll")]
}

pub(crate) fn candidates(name: &str, dirs: &[PathBuf], manifest: Option<PathBuf>) -> Vec<String> {
    let mut found = Vec::new();
    let mut push = |candidate: String| {
        if !found.contains(&candidate) {
            found.push(candidate);
        }
    };
    if let Some(path) = manifest {
        push(path.display().to_string());
    }
    if is_explicit(name) {
        let path = Path::new(name);
        if path.is_relative() {
            for dir in dirs {
                let joined = dir.join(path);
                if joined.exists() {
                    push(joined.display().to_string());
                }
            }
        }
        push(name.to_string());
        return found;
    }
    for dir in dirs {
        for file in local_names(name) {
            let joined = dir.join(file);
            if joined.exists() {
                push(joined.display().to_string());
            }
        }
    }
    for candidate in system_candidates(name) {
        push(candidate);
    }
    found
}

pub(crate) fn open(
    name: &str,
    dirs: &[PathBuf],
    manifest: Option<PathBuf>,
) -> Result<Opened, NotFound> {
    let candidates = candidates(name, dirs, manifest);
    let mut cache = loaded()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut reason = String::from("no candidate file");
    for candidate in &candidates {
        if let Some(library) = cache.get(candidate) {
            return Ok(Opened {
                library,
                path: candidate.clone(),
            });
        }
        match unsafe { Library::new(candidate) } {
            Ok(library) => {
                let library: &'static Library = Box::leak(Box::new(library));
                cache.insert(candidate.clone(), library);
                return Ok(Opened {
                    library,
                    path: candidate.clone(),
                });
            }
            Err(error) => reason = error.to_string(),
        }
    }
    Err(NotFound {
        tried: candidates,
        reason,
    })
}
