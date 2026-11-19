use aelys::{new_vm_with_config, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;
use aelys_runtime::{GcPauseKind, VM, VmConfig};

const PAUSE_CPU_BOUND_NANOS: u64 = 1_500_000;
const PAUSE_WORK_BOUND: u64 = 64_000;

fn run(config: VmConfig, source: &str) -> (i64, VM) {
    let mut vm = new_vm_with_config(config, Vec::new()).expect("vm");
    let value = run_with_vm_and_opt(&mut vm, source, "<gcp>", OptimizationLevel::Standard)
        .unwrap_or_else(|error| panic!("program failed: {error}"));
    (value.as_int().expect("program must return an int"), vm)
}

fn verified() -> VmConfig {
    VmConfig {
        gc_verify: true,
        ..VmConfig::default()
    }
}

fn kinds(vm: &VM) -> String {
    GcPauseKind::ALL
        .iter()
        .map(|kind| {
            let stats = vm.gc_pause_stats().kind(*kind);
            format!(
                "{}: {} pauses, max {} us cpu, max {} work",
                kind.name(),
                stats.count,
                stats.max_cpu_nanos / 1000,
                stats.max_work
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn assert_work_bounded(vm: &VM, what: &str) {
    let worst = vm.gc_pause_stats().max_work();
    assert!(
        worst <= PAUSE_WORK_BOUND,
        "{what}: a pause did {worst} units of work, bound {PAUSE_WORK_BOUND} ({})",
        kinds(vm)
    );
}

const LARGE_BUFFERS: &str = r#"
fn bytes(n: int) -> int {
    let mut b: Vec<u8> = vec![]
    b.resize(n, 7)
    b.write_u8(n - 1, 9)
    b.read_u8(n - 1) + b.read_u8(0)
}
fn ints(n: int) -> int {
    let mut v: Vec<int> = vec![]
    v.resize(n, 3)
    v[n - 1] + v.len()
}
fn text(n: int) -> int {
    let s = "ab".repeat(n)
    s.len()
}
let mut keep: Vec<int> = vec![]
let mut total = 0
for r in 0..4 {
    total = total + bytes(64000000) + ints(4000000) + text(8000000)
    keep.push(total)
}
total + keep.len()
"#;

#[test]
fn large_buffers_freed_by_minor_and_major_collections_stay_within_the_pause_bound() {
    let (value, vm) = run(VmConfig::default(), LARGE_BUFFERS);
    assert_eq!(value, 4 * (16 + 4_000_003 + 16_000_000) + 4);
    assert!(
        vm.heap().minor_collection_count() + vm.heap().major_collection_count() >= 4,
        "the buffers must die in collections ({})",
        kinds(&vm)
    );
    let worst = vm.gc_pause_stats().max_cpu_nanos();
    assert!(
        worst < PAUSE_CPU_BOUND_NANOS,
        "a pause took {} us of thread cpu time ({})",
        worst / 1000,
        kinds(&vm)
    );
}

fn deep_clone(len: i64) -> String {
    format!(
        r#"
struct P {{ x: int, name: string }}
let mut v: Vec<P> = vec![]
for i in 0..{len} {{ v.push(P {{ x: i, name: "p{{i % 10}}" }}) }}
let mut total = 0
for r in 0..3 {{
    let w = v.clone()
    total = total + w[r * 1000].x + w.len() + w[{last}].name.len()
}}
total
"#,
        last = len - 1
    )
}

fn deep_clone_oracle(len: i64) -> i64 {
    (1000 + 2000) + 3 * len + 3 * format!("p{}", (len - 1) % 10).len() as i64
}

#[test]
fn deep_clone_of_a_large_vec_keeps_every_pause_bounded() {
    let (value, vm) = run(VmConfig::default(), &deep_clone(100_000));
    assert_eq!(value, deep_clone_oracle(100_000));
    assert!(vm.heap().minor_collection_count() > 10);
    assert_work_bounded(&vm, "deep clone");
}

#[test]
fn deep_clone_of_a_large_vec_keeps_the_remembered_set_exact_under_verify() {
    for (name, config) in [
        ("verify", verified()),
        (
            "verify small nursery",
            VmConfig {
                gc_nursery_bytes: 8 * 1024,
                ..verified()
            },
        ),
    ] {
        let (value, vm) = run(config, &deep_clone(12_000));
        assert_eq!(value, deep_clone_oracle(12_000), "{name}");
        assert!(vm.heap().minor_collection_count() > 5, "{name}");
    }
}

const LARGE_MAP: &str = r#"
struct P { x: int }
let mut m: Map<int, P> = Map::new()
for i in 0..150000 { let _ = m.insert(i, P { x: i }) }
let mut total = 0
for r in 0..3 {
    let mut c = m.clone()
    for i in 0..150000 {
        if i % 4 == r { let _ = c.insert(i, P { x: i * 2 }) }
        if i % 9 == r { let _ = c.remove(i) }
    }
    for i in 0..150000 {
        match c.get(i) {
            Some(p) => total = total + p.x,
            None => total = total + 1,
        }
    }
}
total
"#;

fn large_map_oracle(len: i64) -> i64 {
    let mut total = 0i64;
    for r in 0..3i64 {
        for i in 0..len {
            total += if i % 9 == r {
                1
            } else if i % 4 == r {
                i * 2
            } else {
                i
            };
        }
    }
    total
}

#[test]
fn large_map_marked_and_rewritten_keeps_every_pause_bounded() {
    let (value, vm) = run(VmConfig::default(), LARGE_MAP);
    assert_eq!(value, large_map_oracle(150_000));
    assert!(
        vm.heap().major_collection_count() > 0,
        "a major must mark the map ({})",
        kinds(&vm)
    );
    assert_work_bounded(&vm, "large map");
}

#[test]
fn large_map_rewritten_with_removals_keeps_the_remembered_set_exact_under_verify() {
    let source = LARGE_MAP.replace("150000", "6000");
    for (name, config) in [
        ("verify", verified()),
        (
            "verify stress",
            VmConfig {
                gc_stress: true,
                gc_nursery_bytes: VmConfig::MIN_GC_NURSERY_BYTES,
                ..verified()
            },
        ),
    ] {
        let (value, vm) = run(config, &source);
        assert_eq!(value, large_map_oracle(6000), "{name}");
        assert!(vm.heap().minor_collection_count() > 5, "{name}");
    }
}

const WAD_LIKE_SIDES: &str = "30000";

const WAD_LIKE: &str = r#"
struct Lump { name: string, pos: int, size: int }
struct Side { xoff: int, yoff: int, upper: string, lower: string, middle: string, sector: int }
struct Pic { width: int, cols: Vec<Vec<int>>, posts: Vec<int> }
struct Level { lumps: Vec<Lump>, sides: Vec<Side>, pics: Vec<Pic>, names: Map<string, int>, lists: Vec<Vec<int>> }

fn tex(i: int) -> string { "TEX{i % 97}_{i % 1000}" }

fn load(buf: Vec<u8>) -> Level {
    let mut lumps: Vec<Lump> = vec![]
    for i in 0..4000 { lumps.push(Lump { name: "L{i}", pos: buf.read_u8(i * 997) + i, size: i * 3 }) }
    let mut sides: Vec<Side> = vec![]
    for i in 0..30000 {
        sides.push(Side { xoff: i, yoff: i * 2, upper: tex(i), lower: tex(i + 1), middle: tex(i + 2), sector: i % 300 })
    }
    let mut pics: Vec<Pic> = vec![]
    for p in 0..120 {
        let mut cols: Vec<Vec<int>> = vec![]
        for c in 0..64 {
            let mut col: Vec<int> = vec![]
            for k in 0..32 { col.push(p + c + k) }
            cols.push(col)
        }
        let mut posts: Vec<int> = vec![]
        for k in 0..2000 { posts.push(k ^ p) }
        pics.push(Pic { width: 64, cols: cols, posts: posts })
    }
    let mut names: Map<string, int> = Map::new()
    for i in 0..4000 { let _ = names.insert(lumps[i].name, i) }
    let mut lists: Vec<Vec<int>> = vec![]
    for b in 0..20000 {
        let mut l: Vec<int> = vec![]
        for k in 0..(b % 7) { l.push(b + k) }
        lists.push(l)
    }
    Level { lumps: lumps, sides: sides, pics: pics, names: names, lists: lists }
}

fn checksum(level: Level) -> int {
    let mut s = 0
    for i in 0..level.lumps.len() { s = (s + level.lumps[i].pos + level.lumps[i].size) % 1000000007 }
    for i in 0..level.sides.len() {
        let side = level.sides[i]
        s = (s + side.xoff + side.yoff + side.sector + side.upper.len() + side.lower.len() + side.middle.len()) % 1000000007
    }
    for p in 0..level.pics.len() {
        let pic = level.pics[p]
        for c in 0..pic.cols.len() { s = (s + pic.cols[c][31]) % 1000000007 }
        s = (s + pic.posts[1999]) % 1000000007
    }
    for b in 0..level.lists.len() { s = (s + level.lists[b].len()) % 1000000007 }
    match level.names.get("L3999") {
        Some(i) => s = (s + i) % 1000000007,
        None => s = s + 0,
    }
    s
}

fn wad() -> Vec<u8> {
    let mut buf: Vec<u8> = vec![]
    buf.resize(64000000, 0)
    for i in 0..4000 { buf.write_u8(i * 997, i % 251) }
    buf
}

let mut directory = "LUMPNAME".repeat(1000000)
let mut level = load(wad())
let first = checksum(level)
let names = directory.len()
directory = "-"
level = load(wad())
let copy = level.sides.clone()
first + checksum(level) + copy.len() + names + directory.len()
"#;

fn wad_like_oracle(sides: i64) -> i64 {
    const M: i64 = 1_000_000_007;
    let tex_len = |i: i64| format!("TEX{}_{}", i % 97, i % 1000).len() as i64;
    let mut s = 0i64;
    for i in 0..4000i64 {
        s = (s + i % 251 + i + i * 3) % M;
    }
    for i in 0..sides {
        s = (s + i + i * 2 + i % 300 + tex_len(i) + tex_len(i + 1) + tex_len(i + 2)) % M;
    }
    for p in 0..120i64 {
        for c in 0..64i64 {
            s = (s + p + c + 31) % M;
        }
        s = (s + (1999 ^ p)) % M;
    }
    for b in 0..20000i64 {
        s = (s + b % 7) % M;
    }
    s = (s + 3999) % M;
    2 * s + sides + 8_000_000 + 1
}

#[test]
fn wad_like_load_builds_the_right_level_with_every_pause_bounded() {
    let (value, vm) = run(VmConfig::default(), WAD_LIKE);
    assert_eq!(value, wad_like_oracle(30_000));
    assert!(vm.heap().major_collection_count() > 0, "{}", kinds(&vm));
    assert_work_bounded(&vm, "wad-like load");
    // unoptimized builds trace about five times slower, while freeing a 64 MB buffer costs the same kernel time in both
    let bound = if cfg!(debug_assertions) {
        4 * PAUSE_CPU_BOUND_NANOS
    } else {
        PAUSE_CPU_BOUND_NANOS
    };
    let worst = vm.gc_pause_stats().max_cpu_nanos();
    assert!(
        worst < bound,
        "a pause took {} us of thread cpu time ({})",
        worst / 1000,
        kinds(&vm)
    );
}

#[test]
fn wad_like_load_keeps_the_remembered_set_exact_under_verify() {
    let source = WAD_LIKE.replace(WAD_LIKE_SIDES, "3000");
    let (value, vm) = run(verified(), &source);
    assert_eq!(value, wad_like_oracle(3000));
    assert!(vm.heap().minor_collection_count() > 5, "{}", kinds(&vm));
}

#[test]
fn every_pause_is_counted_under_its_kind() {
    let (value, vm) = run(VmConfig::default(), &deep_clone(20_000));
    assert_eq!(value, deep_clone_oracle(20_000));
    let stats = vm.gc_pause_stats();
    let by_kind: u64 = GcPauseKind::ALL
        .iter()
        .map(|kind| stats.kind(*kind).count)
        .sum();
    assert_eq!(by_kind, stats.count());
    assert_eq!(
        stats.kind(GcPauseKind::Minor).count,
        vm.heap().minor_collection_count()
    );
    assert_eq!(
        stats.kind(GcPauseKind::MajorFinish).count,
        vm.heap().major_collection_count()
    );
    let report = vm.gc_stats_report();
    assert!(report.contains("pauses by kind minor="), "{report}");
    assert!(report.contains("max_pause_work="), "{report}");
}
