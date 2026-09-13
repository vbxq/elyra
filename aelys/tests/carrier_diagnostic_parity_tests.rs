use aelys::{CompileOptions, Runtime};

const ACCEPTED: &str = "accepted";

struct Cell {
    carrier: &'static str,
    source: String,
    site: &'static str,
}

struct Outcome {
    code: String,
    message: String,
    site_line: Option<String>,
}

fn compile_outcome(cell: &Cell) -> Outcome {
    let Err(error) = Runtime::new().compile(&cell.source, CompileOptions::default()) else {
        return Outcome {
            code: ACCEPTED.to_string(),
            message: String::new(),
            site_line: None,
        };
    };
    let rendered = error.to_string();
    Outcome {
        code: first_code(&rendered),
        message: first_message(&rendered),
        site_line: reported_line(&cell.source, &rendered),
    }
}

fn first_code(rendered: &str) -> String {
    let Some(open) = rendered.find("error[") else {
        return format!("no code in: {rendered}");
    };
    let tail = &rendered[open + "error[".len()..];
    match tail.find(']') {
        Some(close) => tail[..close].to_string(),
        None => format!("no code in: {rendered}"),
    }
}

fn first_message(rendered: &str) -> String {
    let Some(open) = rendered.find("error[") else {
        return rendered.trim().to_string();
    };
    let tail = &rendered[open..];
    let tail = match tail.find("]: ") {
        Some(colon) => &tail[colon + "]: ".len()..],
        None => tail,
    };
    tail.lines().next().unwrap_or("").trim().to_string()
}

fn reported_line(source: &str, rendered: &str) -> Option<String> {
    let arrow = rendered.find("--> ")?;
    let locator = rendered[arrow + "--> ".len()..].lines().next()?;
    let mut parts = locator.rsplitn(3, ':');
    let _column = parts.next()?;
    let line = parts.next()?.parse::<usize>().ok()?;
    source
        .lines()
        .nth(line - 1)
        .map(|line| line.trim().to_string())
}

fn render_grid(family: &str, cells: &[Cell], outcomes: &[Outcome]) -> String {
    let mut grid = format!("grid for {family}:\n");
    for (cell, outcome) in cells.iter().zip(outcomes) {
        grid.push_str(&format!(
            "  {:<28} {:<10} {}\n",
            cell.carrier, outcome.code, outcome.message
        ));
    }
    grid
}

fn assert_parity(family: &str, expected: &str, cells: &[Cell], same_message: bool) {
    let outcomes = cells.iter().map(compile_outcome).collect::<Vec<_>>();
    let grid = render_grid(family, cells, &outcomes);
    let reference = &outcomes[0];
    assert_eq!(
        cells[0].carrier, "free function",
        "the first cell of every family is the free carrier\n{grid}"
    );
    assert_eq!(
        reference.code, expected,
        "the free carrier must raise {expected}\n{grid}"
    );
    for (cell, outcome) in cells.iter().zip(&outcomes).skip(1) {
        assert_eq!(
            outcome.code, reference.code,
            "carrier '{}' must raise the same code as the free carrier\n{grid}",
            cell.carrier
        );
        if same_message {
            assert_eq!(
                outcome.message, reference.message,
                "carrier '{}' must raise the same message as the free carrier\n{grid}",
                cell.carrier
            );
        }
    }
    for (cell, outcome) in cells.iter().zip(&outcomes) {
        let Some(site_line) = &outcome.site_line else {
            panic!(
                "carrier '{}' reported no source location\n{grid}",
                cell.carrier
            );
        };
        assert!(
            site_line.contains(cell.site),
            "carrier '{}' must point at '{}', it pointed at '{site_line}'\n{grid}",
            cell.carrier,
            cell.site
        );
    }
}

const DISPLAY_PRELUDE: &str = "\
struct Pair<A, B> { a: A, b: B }
";

fn display_obligation_cells() -> Vec<Cell> {
    vec![
        Cell {
            carrier: "free function",
            site: "println(p)",
            source: format!(
                "{DISPLAY_PRELUDE}
fn show<T>(x: T) -> int {{
    let p = Pair {{ a: x, b: x }}
    println(p)
    7
}}

fn probe() -> int {{
    show(3)
}}
probe()
"
            ),
        },
        Cell {
            carrier: "impl method, impl param",
            site: "println(p)",
            source: format!(
                "{DISPLAY_PRELUDE}
struct G<T> {{ n: T }}

impl<T> G<T> {{
    fn show(self, x: T) -> int {{
        let p = Pair {{ a: x, b: x }}
        println(p)
        7
    }}
}}

fn probe() -> int {{
    let g = G {{ n: 1 }}
    g.show(3)
}}
probe()
"
            ),
        },
        Cell {
            carrier: "impl method, method param",
            site: "println(p)",
            source: format!(
                "{DISPLAY_PRELUDE}
struct G {{ n: int }}

impl G {{
    fn show<T>(self, x: T) -> int {{
        let p = Pair {{ a: x, b: x }}
        println(p)
        7
    }}
}}

fn probe() -> int {{
    let g = G {{ n: 1 }}
    g.show(3)
}}
probe()
"
            ),
        },
        Cell {
            carrier: "associated function",
            site: "println(p)",
            source: format!(
                "{DISPLAY_PRELUDE}
struct G {{ n: int }}

impl G {{
    fn show<T>(x: T) -> int {{
        let p = Pair {{ a: x, b: x }}
        println(p)
        7
    }}
}}

fn probe() -> int {{
    G::show(3)
}}
probe()
"
            ),
        },
        Cell {
            carrier: "trait method, impl param",
            site: "println(p)",
            source: format!(
                "{DISPLAY_PRELUDE}
struct G<T> {{ n: T }}

trait Shower {{
    fn show(self) -> int;
}}

impl<T> Shower for G<T> {{
    fn show(self) -> int {{
        let p = Pair {{ a: self.n, b: self.n }}
        println(p)
        7
    }}
}}

fn probe() -> int {{
    let g = G {{ n: 1 }}
    g.show()
}}
probe()
"
            ),
        },
        Cell {
            carrier: "lambda in a free function",
            site: "println(p)",
            source: format!(
                "{DISPLAY_PRELUDE}
fn show<T>(x: T) -> int {{
    let render = fn() -> int {{
        let p = Pair {{ a: x, b: x }}
        println(p)
        7
    }}
    render()
}}

fn probe() -> int {{
    show(3)
}}
probe()
"
            ),
        },
        Cell {
            carrier: "lambda in an impl method",
            site: "println(p)",
            source: format!(
                "{DISPLAY_PRELUDE}
struct G<T> {{ n: T }}

impl<T> G<T> {{
    fn show(self, x: T) -> int {{
        let render = fn() -> int {{
            let p = Pair {{ a: x, b: x }}
            println(p)
            7
        }}
        render()
    }}
}}

fn probe() -> int {{
    let g = G {{ n: 1 }}
    g.show(3)
}}
probe()
"
            ),
        },
        Cell {
            carrier: "trait default body",
            site: "println(p)",
            source: format!(
                "{DISPLAY_PRELUDE}
struct G<T> {{ n: T }}

trait Shower {{
    fn item(self) -> int;
    fn show(self) -> int {{
        let p = Pair {{ a: self.item(), b: self.item() }}
        println(p)
        7
    }}
}}

impl<T> Shower for G<T> {{
    fn item(self) -> int {{ 1 }}
}}

fn probe() -> int {{
    let g = G {{ n: 1 }}
    g.show()
}}
probe()
"
            ),
        },
    ]
}

const BOUND_PRELUDE: &str = "\
trait Source {
    fn next(self) -> int;
}

struct Well { depth: int }

impl Source for Well {
    fn next(self) -> int { self.depth }
}

struct Rock { mass: int }
";

fn violated_bound_cells() -> Vec<Cell> {
    vec![
        Cell {
            carrier: "free function",
            site: "u.next()",
            source: format!(
                "{BOUND_PRELUDE}
fn draw<U: Source>(u: U) -> int {{
    u.next()
}}

fn probe() -> int {{
    draw(Rock {{ mass: 2 }})
}}
probe()
"
            ),
        },
        Cell {
            carrier: "impl method, impl param",
            site: "self.unit.next()",
            source: format!(
                "{BOUND_PRELUDE}
struct Tank<U> {{ unit: U }}

impl<U: Source> Tank<U> {{
    fn draw(self) -> int {{
        self.unit.next()
    }}
}}

fn probe() -> int {{
    let t = Tank {{ unit: Rock {{ mass: 2 }} }}
    t.draw()
}}
probe()
"
            ),
        },
        Cell {
            carrier: "impl method, method param",
            site: "u.next()",
            source: format!(
                "{BOUND_PRELUDE}
struct Tank {{ level: int }}

impl Tank {{
    fn draw<U: Source>(self, u: U) -> int {{
        u.next()
    }}
}}

fn probe() -> int {{
    let t = Tank {{ level: 1 }}
    t.draw(Rock {{ mass: 2 }})
}}
probe()
"
            ),
        },
        Cell {
            carrier: "associated function",
            site: "u.next()",
            source: format!(
                "{BOUND_PRELUDE}
struct Tank {{ level: int }}

impl Tank {{
    fn draw<U: Source>(u: U) -> int {{
        u.next()
    }}
}}

fn probe() -> int {{
    Tank::draw(Rock {{ mass: 2 }})
}}
probe()
"
            ),
        },
        Cell {
            carrier: "trait method, impl param",
            site: "self.unit.next()",
            source: format!(
                "{BOUND_PRELUDE}
trait Drawer {{
    fn draw(self) -> int;
}}

struct Tank<U> {{ unit: U }}

impl<U: Source> Drawer for Tank<U> {{
    fn draw(self) -> int {{
        self.unit.next()
    }}
}}

fn probe() -> int {{
    let t = Tank {{ unit: Rock {{ mass: 2 }} }}
    t.draw()
}}
probe()
"
            ),
        },
    ]
}

fn unused_declared_bound_cells() -> Vec<Cell> {
    vec![
        Cell {
            carrier: "free function",
            site: "draw(Rock { mass: 2 })",
            source: format!(
                "{BOUND_PRELUDE}
fn draw<U: Source>(u: U) -> int {{
    1
}}

fn probe() -> int {{
    draw(Rock {{ mass: 2 }})
}}
probe()
"
            ),
        },
        Cell {
            carrier: "impl method, impl param",
            site: "t.draw()",
            source: format!(
                "{BOUND_PRELUDE}
struct Tank<U> {{ unit: U }}

impl<U: Source> Tank<U> {{
    fn draw(self) -> int {{
        1
    }}
}}

fn probe() -> int {{
    let t = Tank {{ unit: Rock {{ mass: 2 }} }}
    t.draw()
}}
probe()
"
            ),
        },
        Cell {
            carrier: "impl method, method param",
            site: "t.draw(Rock { mass: 2 })",
            source: format!(
                "{BOUND_PRELUDE}
struct Tank {{ level: int }}

impl Tank {{
    fn draw<U: Source>(self, u: U) -> int {{
        1
    }}
}}

fn probe() -> int {{
    let t = Tank {{ level: 1 }}
    t.draw(Rock {{ mass: 2 }})
}}
probe()
"
            ),
        },
        Cell {
            carrier: "associated function",
            site: "Tank::draw(Rock { mass: 2 })",
            source: format!(
                "{BOUND_PRELUDE}
struct Tank {{ level: int }}

impl Tank {{
    fn draw<U: Source>(u: U) -> int {{
        1
    }}
}}

fn probe() -> int {{
    Tank::draw(Rock {{ mass: 2 }})
}}
probe()
"
            ),
        },
    ]
}

fn unresolved_parameter_cells() -> Vec<Cell> {
    vec![
        Cell {
            carrier: "free function",
            site: "drain(5)",
            source: format!(
                "{BOUND_PRELUDE}
fn drain<U: Source>(n: int) -> int {{
    n
}}

fn probe() -> int {{
    drain(5)
}}
probe()
"
            ),
        },
        Cell {
            carrier: "impl method, method param",
            site: "t.drain(5)",
            source: format!(
                "{BOUND_PRELUDE}
struct Tank {{ level: int }}

impl Tank {{
    fn drain<U: Source>(self, n: int) -> int {{
        n
    }}
}}

fn probe() -> int {{
    let t = Tank {{ level: 1 }}
    t.drain(5)
}}
probe()
"
            ),
        },
        Cell {
            carrier: "associated function",
            site: "Tank::drain(5)",
            source: format!(
                "{BOUND_PRELUDE}
struct Tank {{ level: int }}

impl Tank {{
    fn drain<U: Source>(n: int) -> int {{
        n
    }}
}}

fn probe() -> int {{
    Tank::drain(5)
}}
probe()
"
            ),
        },
    ]
}

fn growing_recursion_cells() -> Vec<Cell> {
    vec![
        Cell {
            carrier: "free function",
            site: "grow(p, depth - 1)",
            source: format!(
                "{DISPLAY_PRELUDE}
fn grow<T>(x: T, depth: int) -> int {{
    if depth <= 0 {{
        0
    }} else {{
        let p = Pair {{ a: x, b: x }}
        grow(p, depth - 1)
    }}
}}

fn probe() -> int {{
    grow(1, 5)
}}
probe()
"
            ),
        },
        Cell {
            carrier: "impl method, impl param",
            site: "g.grow(depth - 1)",
            source: format!(
                "{DISPLAY_PRELUDE}
struct G<T> {{ n: T }}

impl<T> G<T> {{
    fn grow(self, depth: int) -> int {{
        if depth <= 0 {{
            0
        }} else {{
            let p = Pair {{ a: self.n, b: self.n }}
            let g = G {{ n: p }}
            g.grow(depth - 1)
        }}
    }}
}}

fn probe() -> int {{
    let g = G {{ n: 1 }}
    g.grow(5)
}}
probe()
"
            ),
        },
        Cell {
            carrier: "associated function",
            site: "G::grow(p, depth - 1)",
            source: format!(
                "{DISPLAY_PRELUDE}
struct G {{ n: int }}

impl G {{
    fn grow<T>(x: T, depth: int) -> int {{
        if depth <= 0 {{
            0
        }} else {{
            let p = Pair {{ a: x, b: x }}
            G::grow(p, depth - 1)
        }}
    }}
}}

fn probe() -> int {{
    G::grow(1, 5)
}}
probe()
"
            ),
        },
    ]
}

#[test]
fn a_display_obligation_is_the_same_diagnostic_on_every_carrier() {
    assert_parity(
        "E0338, the Display obligation raised inside a generic body",
        "E0338",
        &display_obligation_cells(),
        true,
    );
}

#[test]
fn a_violated_declared_bound_is_the_same_diagnostic_on_every_carrier() {
    assert_parity(
        "E0338, a declared bound violated at the instantiation site",
        "E0338",
        &violated_bound_cells(),
        true,
    );
}

#[test]
fn an_unused_declared_bound_is_the_same_diagnostic_on_every_carrier() {
    assert_parity(
        "E0338, a declared bound no body ever uses",
        "E0338",
        &unused_declared_bound_cells(),
        true,
    );
}

#[test]
fn an_unresolved_type_parameter_is_the_same_diagnostic_on_every_carrier() {
    assert_parity(
        "E0343, a type parameter nothing binds",
        "E0343",
        &unresolved_parameter_cells(),
        true,
    );
}

#[test]
fn a_growing_recursion_is_the_same_diagnostic_on_every_carrier() {
    assert_parity(
        "E0344, a generic recursion that never decreases",
        "E0344",
        &growing_recursion_cells(),
        false,
    );
}
