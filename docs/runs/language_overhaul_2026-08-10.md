# aelys language overhaul

## run state

- date: 2026-08-10
- repository: `/home/vbxq/sources/elyra`, branch `master`, head `4718fb4`
- consumer: `/home/vbxq/sources/raizen_core`, branch `main`, existing local changes preserved
- push: forbidden
- worktrees: none
- current phase: chantier 1 closed, chantier 2 design gate pending

## stage 0

### evidence

The consumer was run from its working tree with one invocation of `cargo test --workspace`.
The result was 150 passed and 0 failed: 25 asset tests, 94 engine unit tests, 4 binary
tests, 12 real asset tests, 7 extract tests, and 8 stdlib tests. This satisfies the
required 146 or more passed tests with no failures.

`cargo xtask ci` was then run in elyra. It passed format, Clippy with `-D warnings`,
the workspace test suite with all features, and the diff check.

### repairs needed for ci

HEAD had three Clippy blockers unrelated to the language surface:

- public unsafe native descriptor entrypoints lacked safety sections
- the heap deliberately stores each slot in a `Box` because execution caches pointers into objects
- v2 JIT code and one test had current Clippy violations

The repairs use doc attributes for the required safety sections, a targeted Clippy
allow for the pointer-stability representation, clearer control flow, and an `assert!`.
No bytecode or runtime behavior changed. The consumer's existing dirty changes were
not rewritten.

### test red evidence

No new test was written during stage 0. The existing consumer and elyra suites were
run only after the CI repairs and were green.

## next action

Inventory the lexer, parser, AST, type checker, compiler, VM, diagnostics, and all
seven `raizen-scripts/*.aelys` files. Establish the current accepted forms and add
compile-fail tests with named diagnostics before implementing `::` paths.

## chantier 1 test red

Two path tests were added before implementation and run independently.

- `test_module_path_uses_double_colon` failed at `needs helpers::math` with
  `UnexpectedToken { expected: "semicolon or newline", found: ":" }`.
- `test_dot_module_member_reports_the_path_fix` failed because the old
  `sys.arch()` form was accepted and did not produce the required teaching diagnostic.

The first failure proves the lexer and parser had no `::` token. The second proves
the existing module dispatch silently treated a dot as a module separator.

## chantier 1 close

`ColonColon` is now a distinct token. A path member is carried through the AST, typed
AST, optimizer, and both compiler paths; value members retain `.`. Module imports reject
the old dotted form with diagnostic E0411:
`module members are reached with '::'; write 'sys::arch'`.

The module-call resolver accepts a complete path, not only one separator. The native
fixture exports `b::c`; `native_test::b::c(5, 5)` executed and returned the expected
value. The seven consumer scripts, all elyra examples, and affected embedded Aelys
sources were migrated. No old module selector remains in the seven scripts.

The consumer was first run against the old git dependency after the new consumer
compile-fail test was written. `old_module_member_syntax_reports_the_path_migration`
went red because `raizen.add_score(1)` was accepted and returned `()`. The path
dependency was restored, the diagnostic assertion passed, and the complete consumer
`cargo test --workspace` then reported 146 passed and 0 failed, including the seven
script compile/run paths and the stage-one timeline. The final elyra `cargo xtask ci`
also passed format, Clippy, all-feature tests, and diff check.

No bytecode, value representation, or VM dispatch redesign was needed for this
chantier; there is no rebuild measurement to report.

## next action after chantier 1

Open the unbounded error-handling design gate. Before implementation, resolve the
closed versus open error model, `?` conversion, match exhaustivity domain, unused
`Result` enforcement, and the representation cost. Record each review round and its
rejected alternatives here before touching the implementation.
