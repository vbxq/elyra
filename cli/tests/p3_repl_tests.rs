use std::io::Write;
use std::process::{Command, Stdio};

fn repl(flags: &[&str], input: &str) -> Vec<String> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("repl")
        .args(flags)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the repl");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("feed the repl");
    let output = child.wait_with_output().expect("repl exits");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .expect("utf8")
        .lines()
        .map(str::to_string)
        .collect()
}

fn values(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| line.as_str() != "()")
        .cloned()
        .collect()
}

#[test]
fn a_function_defined_on_one_line_is_called_on_later_lines() {
    for flags in [&["-O0"][..], &["-O2"][..], &["-O2", "-ae.jit=on"][..]] {
        let lines = repl(
            flags,
            "fn f(a: int) -> int { a * 2 }\nf(4)\nfn g(b: int) -> int { f(b) + 1 }\ng(10)\nf(g(1))\nexit\n",
        );
        assert_eq!(values(&lines), ["8", "21", "6"], "{flags:?}: {lines:?}");
    }
}

#[test]
fn structs_enums_impls_traits_and_constants_persist_across_lines() {
    let lines = repl(
        &[],
        "struct P { a: int }\nimpl P { fn twice(self) -> int { self.a * 2 } }\nlet p = P { a: 3 }\np.a + p.twice()\nenum E { A(int), B }\nlet e = E::A(5)\nmatch e { E::A(x) => x, E::B => 0 }\ntrait Area { fn area(self) -> int; }\nimpl Area for P { fn area(self) -> int { self.a * self.a } }\np.area()\nconst K = 7\nK * 2\nfn mk(n: int) -> P { P { a: n + K } }\nmk(1).twice()\n",
    );
    assert_eq!(values(&lines), ["9", "5", "9", "14", "16"], "{lines:?}");
}

#[test]
fn globals_of_every_shape_keep_their_type_on_later_lines() {
    let lines = repl(
        &[],
        "let s = \"abc\"\ns.len()\nlet v = vec![1, 2, 3]\nv[2] + v.len()\nlet o: Option<int> = Some(4)\no.unwrap_or(0)\nlet t = (5, \"x\")\nt.0\nlet mut n = 1\nn = n + 10\nn\nstruct B<T> { v: T }\nlet b = B { v: 6 }\nb.v\nlet sq = fn(x: int) -> int { x * x }\nsq(7)\nlet m: Map<string, int> = Map::new()\nm.len()\n",
    );
    assert_eq!(
        values(&lines),
        ["3", "6", "4", "5", "11", "6", "49", "0"],
        "{lines:?}"
    );
}

#[test]
fn a_function_reads_and_writes_a_global_defined_on_an_earlier_line() {
    let lines = repl(
        &[],
        "let mut count = 0\nfn bump() { count = count + 1 }\nbump()\nbump()\ncount\nfn total(x: int) -> int { x + count }\ntotal(40)\n",
    );
    assert_eq!(values(&lines), ["2", "42"], "{lines:?}");
}

#[test]
fn a_redefined_function_replaces_the_earlier_one_for_every_caller() {
    let lines = repl(
        &[],
        "fn f() -> int { 1 }\nfn g() -> int { f() * 10 }\ng()\nfn f() -> int { 2 }\ng()\nf()\n",
    );
    assert_eq!(values(&lines), ["10", "20", "2"], "{lines:?}");
}

#[test]
fn a_rejected_definition_is_not_kept_and_errors_point_at_the_current_line() {
    let lines = repl(
        &[],
        "fn ok() -> int { 1 }\nfn bad() -> int { \"s\" }\nbad()\nok() + true\nok()\n",
    );
    let text = lines.join("\n");
    assert!(text.contains("E0301"), "{text}");
    assert!(text.contains("undefined variable: bad"), "{text}");
    assert!(text.contains("--> <repl>:1:1"), "{text}");
    assert!(!text.contains("<repl>:2:"), "{text}");
    assert_eq!(lines.last().map(String::as_str), Some("1"), "{text}");
}

#[test]
fn a_runtime_error_on_a_later_line_names_that_line() {
    let lines = repl(
        &[],
        "fn one() -> int { 1 }\nfn two() -> int { 2 }\nlet z = 0\n5 / z\none() + two()\n",
    );
    let text = lines.join("\n");
    assert!(text.contains("division by zero"), "{text}");
    assert!(text.contains("--> <repl>:1:1"), "{text}");
    assert_eq!(lines.last().map(String::as_str), Some("3"), "{text}");
}
