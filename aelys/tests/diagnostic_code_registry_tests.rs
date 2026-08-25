
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const COMMON_CODES: &str = "common/src/error/compile/code.rs";
const SEMA_CODES: &str = "sema/src/constraint/error.rs";
const SPEC: &str = "docs/language-spec.md";

const COMMON_FN: &str = "fn code(&self) -> u16 {";
const SEMA_FN: &str = "fn diagnostic_code(&self) -> u16 {";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the aelys crate always sits one level below the workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// drops line comments so that a comma or a brace inside one cannot end an arm
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn match_body(source: &str, header: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut search = 0usize;
    while let Some(found) = source[search..].find(header) {
        let after = search + found + header.len();
        search = after;
        let Some(offset) = source[after..].find("match self {") else {
            continue;
        };
        let open = after + offset + "match self {".len();
        let chars: Vec<char> = source[open..].chars().collect();
        let mut depth = 1i32;
        let mut end = None;
        for (i, c) in chars.iter().enumerate() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(end) = end {
            bodies.push(chars[..end].iter().collect());
        }
    }
    bodies
}

fn arms(body: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = body.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut arrow: Option<usize> = None;
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            '=' if depth == 0 && chars.get(i + 1) == Some(&'>') => {
                arrow = Some(i);
                i += 1;
            }
            ',' if depth == 0 => {
                if let Some(at) = arrow {
                    let pattern: String = chars[start..at].iter().collect();
                    let value: String = chars[at + 2..i].iter().collect();
                    out.push((pattern.trim().to_string(), value.trim().to_string()));
                }
                arrow = None;
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn variant_names(pattern: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = pattern;
    while let Some(at) = rest.find("Self::") {
        rest = &rest[at + "Self::".len()..];
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if end > 0 {
            names.push(rest[..end].to_string());
        }
        rest = &rest[end..];
    }
    names
}

fn source_arms(relative: &str, header: &str) -> Vec<(u16, Vec<String>)> {
    let text = strip_line_comments(&read(relative));
    let bodies = match_body(&text, header);
    let body = bodies
        .into_iter()
        .max_by_key(|b| arms(b).len())
        .unwrap_or_else(|| panic!("no `match self` body found under `{header}` in {relative}"));
    let mut out = Vec::new();
    for (pattern, value) in arms(&body) {
        let Ok(code) = value.parse::<u16>() else {
            continue;
        };
        let names = variant_names(&pattern);
        assert!(
            !names.is_empty(),
            "arm `{pattern}` in {relative} names no `Self::` variant"
        );
        out.push((code, names));
    }
    assert!(
        !out.is_empty(),
        "{relative} yielded no numeric diagnostic codes; the parser or the file shape changed"
    );
    out
}

fn all_source_codes() -> BTreeMap<u16, BTreeSet<String>> {
    let mut map: BTreeMap<u16, BTreeSet<String>> = BTreeMap::new();
    for (code, names) in source_arms(COMMON_CODES, COMMON_FN)
        .into_iter()
        .chain(source_arms(SEMA_CODES, SEMA_FN))
    {
        map.entry(code).or_default().extend(names);
    }
    map
}

fn documented_codes() -> Vec<(u16, String, usize)> {
    let spec = read(SPEC);
    let mut out = Vec::new();
    for (index, line) in spec.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() < 3 {
            continue;
        }
        let Some(digits) = cells[0].strip_prefix('E') else {
            continue;
        };
        if digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(code) = digits.parse::<u16>() else {
            continue;
        };
        if !cells[1].chars().all(|c| c.is_alphanumeric() || c == '_') || cells[1].is_empty() {
            continue;
        }
        out.push((code, cells[1].to_string(), index + 1));
    }
    out
}

#[test]
fn no_source_file_hands_one_code_to_two_diagnostics() {
    for (relative, header) in [(COMMON_CODES, COMMON_FN), (SEMA_CODES, SEMA_FN)] {
        let mut seen: BTreeMap<u16, Vec<String>> = BTreeMap::new();
        for (code, names) in source_arms(relative, header) {
            seen.entry(code).or_default().push(names.join(" | "));
        }
        let collisions: Vec<String> = seen
            .iter()
            .filter(|(_, arms)| arms.len() > 1)
            .map(|(code, arms)| format!("E{code:04} is used by {} arms: {arms:?}", arms.len()))
            .collect();
        assert!(
            collisions.is_empty(),
            "{relative} reuses diagnostic codes:\n{}",
            collisions.join("\n")
        );
    }
}

#[test]
fn the_registry_lists_every_code_exactly_once() {
    let documented = documented_codes();
    assert!(
        documented.len() > 100,
        "only {} registry rows parsed out of docs/language-spec.md; the table shape changed",
        documented.len()
    );
    let mut seen: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (code, _, line) in &documented {
        seen.entry(*code).or_default().push(*line);
    }
    let repeats: Vec<String> = seen
        .iter()
        .filter(|(_, lines)| lines.len() > 1)
        .map(|(code, lines)| format!("E{code:04} on lines {lines:?}"))
        .collect();
    assert!(
        repeats.is_empty(),
        "the diagnostic code registry lists a code more than once:\n{}",
        repeats.join("\n")
    );
}

#[test]
fn every_documented_code_exists_in_the_source() {
    let source = all_source_codes();
    let mut missing = Vec::new();
    for (code, name, line) in documented_codes() {
        match source.get(&code) {
            None => missing.push(format!(
                "E{code:04} ({name}) on line {line} of {SPEC} is assigned by no source arm"
            )),
            Some(names) if !names.contains(&name) => missing.push(format!(
                "E{code:04} on line {line} of {SPEC} is documented as `{name}` but the source \
                 assigns it to {names:?}"
            )),
            Some(_) => {}
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

#[test]
fn every_source_code_is_documented() {
    let documented: BTreeSet<u16> = documented_codes().into_iter().map(|(c, _, _)| c).collect();
    let undocumented: Vec<String> = all_source_codes()
        .into_iter()
        .filter(|(code, _)| !documented.contains(code))
        .map(|(code, names)| format!("E{code:04} {names:?}"))
        .collect();
    assert!(
        undocumented.is_empty(),
        "these diagnostic codes exist in the source but are absent from the registry in {SPEC}:\n{}",
        undocumented.join("\n")
    );
}
