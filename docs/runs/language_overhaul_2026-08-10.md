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

## chantier 2 design round 1

This is a design record only. No error-handling implementation is allowed until a
hostile review has rejected the holes below and a later round is marked PROVEN.

### assertion 0

Match exhaustivity, unused `Result` and `Option`, the absence of `null` from the
surface language, and compile-time validation of every `?` conversion are hard
requirements. A dynamic fallback, a VM check, or a warning does not satisfy them.

### error model

`Result<T, E>` and `Option<T>` are first-class nominal sum types in the inference
and resolved-type graphs. The syntax accepts multiple type arguments, so
`Result<int, string>` is not represented as a special two-field tuple. `Ok`, `Err`,
`Some`, and `None` are compiler-known constructors. `None`, `Ok`, and `Err` must
receive a type from an annotation, a function return, a parameter, or a match
context; an unresolved constructor is a compile error.

The result payload type `E` remains generic and may be any statically known type.
Stage 1 does not expose user-defined error traits or user-defined data enums. It
provides one closed built-in `Error` family for standard-library failures, with the
conversion constructor `Error::Message(string)`. User-defined error families and a
trait-based conversion relation belong to Stage 2, with an explicit owner in the
ledger rather than an implicit dynamic escape hatch.

`?` has exactly this conversion relation in Stage 1:

- `Option<T>?` is valid only in a function returning `Option<U>` and propagates
  `None` while yielding `T` on `Some`.
- `Result<T, E>?` is valid only in a function returning `Result<U, F>`.
- The error is passed unchanged when `E` and `F` are identical.
- `string` converts to the built-in `Error` through `Error::Message`; this is the
  only non-identity conversion.
- Option residuals never become Result errors, arbitrary `E` to `F` conversion is
  never guessed, and dynamic types are rejected at the `?` site.

The rejected alternatives are an open `From`-like trait before traits exist, which
would turn a missing implementation into a runtime conversion, and implicit
stringification of every error type, which would erase type mismatches. The
diagnostic for a rejected conversion names both types and recommends `map_err`.

### patterns and exhaustivity

`match` is an expression. An arm has a pattern, an optional `if` guard, `=>`, and an
expression or a block whose final expression is the arm value. Patterns support
bindings, `_`, nested `Some`, `None`, `Ok`, `Err`, literals, guards, and `|`
alternation. A path in a variant pattern uses `::`; a value member remains `.`.

Stage 1 checks exhaustivity for `Option`, `Result`, and `bool`. A wildcard or
binding covers the whole scrutinee. A guarded arm never contributes coverage.
Alternation contributes the union of its unguarded alternatives. Nested coverage is
recursive, so `Some(true)` does not cover `Some(false)`. A non-exhaustive sum match
is the named compile error `NonExhaustiveMatch` and includes the missing variants
and a fix such as `add Err(error) or _`.

Integer, string, and dynamic matches require an unguarded wildcard or binding. The
compiler does not pretend that a finite list of literals covers an infinite domain.
User-defined enum and struct pattern coverage is Stage 2 work; accepting it as a
dynamic pattern now is rejected rather than silently unchecked.

### must use

Both `Result` and `Option` carry a compile-time must-use effect. A direct expression
statement whose resolved type is either sum is rejected with `IgnoredResult` or
`IgnoredOption`, including a final top-level expression and a non-tail expression in
a function. A tail expression is used only when it is the function's implicit
return. Returning, binding, passing, matching, applying `?`, or calling a consuming
method counts as use. `let _ = expression` is the one explicit discard form and is
accepted. The check runs after substitution, so a call that was dynamic during the
first inference pass cannot evade it after resolving to a sum.

The must-use pass is context aware. It marks the final expression of an implicit
return and the final expressions of both branches of an implicit-return `if` or
`match` as used; every other statement expression is discarded context. It also
walks block arms instead of treating a block as an opaque side effect.

### null and unit

`null` is removed from the surface grammar as a value and as a type. The lexer keeps
its token only long enough to emit the named `NullIsNotInSurface` diagnostic, which
says to choose `Option` for absence or `Result` for failure. `()` is the unit value
for functions and native calls with no result. The old NaN-box `Value::null`, the
legacy `LoadNull` opcode, and host ABI null handling remain private compatibility
sentinels until all host tests are migrated; the Aelys parser, typed AST, compiler,
standard modules, and consumer scripts cannot construct or receive them.

Standard-library absence and failure paths are migrated to `Option` and `Result`.
Void exports return unit. The native boundary normalizes a legacy null returned by
an untyped host export to unit before it can enter Aelys, so an accidental host
sentinel cannot become a surface value. Typed standard exports are registered with
their Aelys signatures; an unknown native export is a dynamic FFI boundary and
cannot be used with `?` or match without an explicit typed wrapper.

### representation and bytecode

The existing NaN-box remains the outer `Value` representation. A sum value is one
GC object containing a family tag, a variant tag, an arity, and up to one `Value`
payload. Payloads are already full 64-bit values, so `Ok(42)` does not box the
integer separately. `None` and unit use the two unused NaN-box tags and allocate no
object. A multi-value built-in error is represented by one tuple-like payload or
by a single formatted message, never by an untracked Rust pointer.

The heap walker marks every payload value in a sum object, and allocation sizing
counts the object and its payload. The sum object is the only new GC root edge.
This avoids changing AVBC constant serialization and avoids boxing every primitive
result. The compiler adds direct sum construction, variant-test, and payload-load
instructions rather than routing match and `?` through user-visible dynamic calls.
Their narrow and wide encodings follow the existing register rules. The JIT rejects
these instructions until it has a type-safe translation; it must fall back to the
interpreter without treating an unknown opcode as success.

The before measurement is the compiled byte count and interpreter dispatch count
for `raizen-scripts/stage1.aelys` at the chantier 1 commit. The after measurement
is the same script and same harness after the new opcodes and value tags. A change
to either number, heap allocation count, or dispatch count is recorded before the
phase closes. No AVBC version bump or representation rewrite is justified until
that measurement shows a real need.

### implementation and proof order

The implementation must land in this order: named diagnostics and compile-fail
tests; token and AST patterns; generic type annotations and sum inference; the
must-use and exhaustivity pass; sum object and immediate values; direct bytecode
and VM operations; constructors, `?`, methods, and match code generation; standard
library migration; consumer script migration.

Every rejection test is first run against the unfixed tree and its red output is
recorded, then the implementation is restored and the named diagnostic is asserted.
The minimum rejection matrix is: missing `Err`, missing `None`, guard-only
coverage, invalid nested binding, invalid `?` return type, non-identity `?`
conversion, ignored direct `Result`, ignored direct `Option`, unresolved `None`,
surface `null`, and a legacy dot variant path. Runtime tests must execute top-level
code or invoke a function explicitly. A `main` definition alone is not evidence.

## chantier 2 hostile review of round 1

### assertion 0

Match exhaustivity, unused `Result` and `Option`, no surface `null`, and compile-time
`?` compatibility remain non-negotiable. This review treats any dynamic fallback as
failure, even when the runtime has a defensive check.

Round 1 is not PROVEN. The following holes must be closed in the next design round.

1. Alternation bindings were underspecified. `Ok(x) | Err(x)` must bind the same
   names with compatible types on every alternative, and the compiler must reject
   `Ok(x) | Err(y)` rather than generating an uninitialized register. A guard must
   be type checked as `bool`, and a guard must never contribute exhaustivity.

2. Nested coverage needs a finite-domain algorithm, not an outer-variant bitset.
   `Some(true)` leaves `Some(false)` uncovered, and
   `Ok(Some(value)) | Err(error)` leaves `Ok(None)` uncovered. The checker must
   recursively expand only closed domains and require a wildcard for an infinite
   payload. The compiler must emit an unreachable trap after an exhaustive match,
   not a null value, for a corrupted or foreign runtime tag.

3. Constructor inference needs an explicit unresolved-state error. `let x = Ok(1)`
   cannot silently become `Result<int, dynamic>`, and `let x = None` cannot become
   `Option<dynamic>`. The constraint solver must preserve the sum family and both
   parameters through substitution, and the finalizer must reject an unresolved
   constructor with a named diagnostic.

4. The conversion relation must be implemented as a checked residual constraint.
   If the target error type is still a type variable, identity is preferred and the
   target is bound to the source error. Only an explicit `Result<_, Error>` target
   enables `string` to `Error::Message`. A dynamic source or target must produce a
   compile error, never a runtime conversion attempt.

5. The null plan was too permissive at the native boundary. Mapping every legacy
   null to unit can hide an absence bug. Each standard export that can be absent or
   fail must be assigned an `Option` or `Result` signature and return that sum; only
   a declared void export may normalize a legacy sentinel to unit. An untyped custom
   native is an unsafe FFI boundary and must be rejected from typed sum operations,
   not treated as proof of a non-null result.

6. The method surface needs a static signature table. `map`, `map_err`,
   `and_then`, `or_else`, `unwrap_or_else`, `ok`, and `err` must constrain closure
   arity, input type, output type, and laziness. A method with the wrong family,
   wrong arity, or wrong callback result must fail in sema. Runtime method dispatch
   cannot be the first place that discovers these errors.

7. The direct bytecode plan must cover verifier, serializer, disassembler, normal
   and wide dispatch, GC tracing, and JIT fallback. An unrecognized sum opcode must
   be rejected by verification, and a recognized but untranslated opcode must make
   JIT compilation decline before execution. A sum payload must be marked through
   the object graph, including a payload allocated immediately before collection.

8. Unit must be a real typed value. Keeping `InferType::Null` in return inference,
   `Return0`, or the standard-library signatures would leave a path for a surface
   null after the parser change. The next round must name every remaining null use
   as host-only, replace source-visible no-value returns with unit, and add a test
   that compiles and executes a void function while asserting it cannot produce the
   null tag.

9. A match arm block cannot be opaque to must-use analysis. The next design must
   define tail position for expression arms, block arms, implicit-return `if`, and
   implicit-return `match`, and must reject a sum expression in every non-tail
   statement position. A `return` in an arm needs a typed never/control-flow path,
   or a named rejection; it cannot be represented by an implicit null.

10. The rejection tests need the unfixed RED output before implementation. In
    particular, tests must prove that the old compiler accepts a missing `Err`, an
    ignored `Result`, a bad `?` conversion, and `null` before the new diagnostics
    are installed. A green test after adding the assertion is not evidence without
    that deliberately observed failure.

## chantier 2 design round 2

Round 2 incorporates the hostile findings. It is still a proposal until the next
review explicitly checks each static gate against this version.

### exact type surface

`TypeAnnotation` stores `type_params: Vec<TypeAnnotation>`. `Option<T>`,
`Result<T, E>`, and the built-in `Error` are the only sum families in Stage 1.
`Error` has one data-carrying constructor, `Error::Message(string)`, so the
conversion relation has a concrete target without pretending that user enums or
traits already exist. `Result<T, E>` still accepts arbitrary known `E`; only the
conversion relation is closed.

`InferType` and `ResolvedType` gain `Unit`, `Option`, `Result`, and `Error` cases.
The unifier, occurs check, substitution, display, type table, typed AST, liveness
walkers, and backend matches must handle all of them. `Dynamic` never satisfies a
sum-family test, a `?` residual, or a match scrutinee requirement. It remains an
explicit FFI boundary for ordinary arithmetic and calls, not a proof of a type.

Constructors are type-directed. `Some(value)` fixes `Option<T>`, `Ok(value)` fixes
the success type and leaves `E` constrained by context, `Err(value)` fixes `E` and
leaves `T` constrained by context, and `None` leaves both the family and `T`
constrained. Finalization emits `UnresolvedSumType` if any constructor still has a
free type variable or dynamic parameter. `Result::Ok`, `Result::Err`,
`Option::Some`, and `Option::None` are accepted aliases of the bare constructors;
an unrelated path is `UnknownVariant`.

### checked residuals

Inference records `TryResidual { source, target, span }` instead of deciding a
conversion from a dynamic or partially solved type. After substitution, it checks
the following closed table:

| source | enclosing return | expression value | propagation |
| --- | --- | --- | --- |
| `Option<T>` | `Option<U>` | `T`, constrained to `U` where used | `None` |
| `Result<T, E>` | `Result<U, F>` with `E == F` | `T`, constrained to `U` where used | original `Err` |
| `Result<T, string>` | `Result<U, Error>` | `T`, constrained to `U` where used | `Err(Error::Message(e))` |

Every other row is `QuestionMarkTypeMismatch`, including a top-level `?`, an
Option-to-Result residual, a dynamic source or target, and a target whose error
type cannot be resolved. The diagnostic prints the two result types and the
explicit `map_err` or `ok_or` style operation that fixes it. Identity is selected
before the string-to-Error row, so an unconstrained target error becomes `string`
rather than silently becoming `Error`.

### pattern binding and coverage

Patterns are represented independently of expressions:

- `Wildcard` and `Binding(name)` cover the entire type and bind nothing or the
  whole value.
- `Literal(bool, int, string)` covers a literal domain member.
- `Variant(path, fields)` names a known family constructor and recursively carries
  patterns for its payload.
- `Or(alternatives)` is the union of alternatives.

The checker first type checks every pattern and guard. Or-patterns must bind the
same set of names in every alternative, with the same inferred type for each name.
Bindings are scoped to the arm and are initialized only after the complete pattern
test succeeds. A guard is required to be `bool` and is excluded from coverage.

Exhaustivity uses recursive usefulness checking over constructor matrices, not an
outer-variant bitset. The finite constructors are `Some` and `None`, `Ok` and
`Err`, and `true` and `false`. A payload of a sum or bool is recursively
specialized; an integer, string, struct, or dynamic payload is considered infinite
and only `_` or a binding covers it. Thus `Some(true)` leaves `Some(false)` and
`Ok(Some(_))` leaves `Ok(None)` visible in the missing-pattern set. The final
diagnostic lists a concrete missing pattern when one exists. A match on an integer,
string, struct, or dynamic value is accepted only when it contains an unguarded
wildcard or binding. Unknown or user-defined enum variants are rejected with
`UnknownVariant` until Stage 2 owns them.

After all statically valid arms, the compiler emits an unreachable trap for an
invalid runtime family or tag. It never loads null as a default arm. This trap is a
defensive check for corrupted bytecode or a foreign host value, not the exhaustivity
mechanism.

### must-use and control flow

The sema pass runs after substitution with a `UseContext` of `Consumed`,
`ImplicitReturn`, or `Discarded`. Any expression whose final type is `Result` or
`Option` is legal in `Consumed` and `ImplicitReturn`, and rejected in `Discarded`
unless it is the initializer of `let _ =`. Calls, returns, arguments, conditions
after type checking, match scrutinees, and `?` operands are consumed. A sum returned
from a combinator remains must-use until its parent consumes it.

The pass propagates `ImplicitReturn` through a function's final expression, an
implicit-return `if` with both branches, and a match arm's final expression or
block. A block ending in `return`, `break`, or `continue` has a `Never` control-flow
result and does not manufacture a unit value. A match arm block with neither a
value nor a diverging statement is `MatchArmValueRequired`. Top-level final
expressions are discarded because module execution does not implicitly return them.

`IgnoredResult` and `IgnoredOption` are fatal named diagnostics, not warnings. The
diagnostic says that the value carries a failure or absence and offers `let _ =`,
`return`, `match`, or a consuming method. This pass is separate from variable-use
warnings so an optimizer cannot erase the check.

### null and native boundary

`Unit` is parsed from `()`, inferred for `return` without an expression and for an
empty or no-value function, and emitted by `Return0` and `LoadUnit`. `null` remains
a lexer token solely so the parser can issue `NullIsNotInSurface`; it is not an AST
literal or a type annotation. `InferType::Null` and `ResolvedType::Null` are removed
from source inference. `Value::null` and legacy bytecode null are host-only and are
never emitted by the Aelys compiler.

Every standard native export has a declared Aelys signature in the standard-module
signature table. Void signatures return unit. Absence signatures return
`Option<T>`, and failure signatures return `Result<T, Error>`. The standard native
implementations construct those values before returning. A legacy null is
normalized only for a signature declared `Unit`; an untyped custom export is not
eligible for `?`, match, or a sum method until wrapped by a typed Aelys function.
This keeps the dynamic boundary explicit instead of using it to prove non-null.

### sum operations and dispatch

The bytecode additions are `LoadUnit`, `LoadNone`, `MakeOptionSome`,
`MakeResultOk`, `MakeResultErr`, `MakeErrorMessage`, `SumTest`, and `SumPayload`.
Each has a verifier rule, a compact and wide register form where needed, assembler
and disassembler support, and an interpreter implementation. Sum construction puts
the family and variant in a GC object and stores one `Value` payload. None and unit
are immediate NaN-box tags. The heap traces the payload through `Value::as_ptr`, and
the size estimator includes the object header and payload.

The compiler lowers `?`, match tests, and all sum methods to these operations plus
ordinary calls and jumps. `unwrap` and `expect` branch to a structured runtime
`UnwrapFailed` error, never to null. `unwrap_or_else`, `and_then`, and `or_else`
place the callback call only on the branch where it is needed. The static method
table supplies the callback and result constraints before lowering.

JIT translation recognizes the new load and test operations only once it can carry
their object tag semantics. Until then, the translator returns an explicit
unsupported result and the runtime uses the verified interpreter path. The JIT
must not emit a default value for an unknown sum operation.

### proof gate before implementation

The next reviewer must independently answer yes to every item below before this
section can be marked PROVEN: missing `Err`, missing `None`, guarded-only coverage,
nested coverage, mismatched alternation bindings, unresolved constructors, invalid
`?` conversion, ignored sums, `null`, wrong sum method signatures, non-value match
arms, invalid runtime tags, GC payload marking, verifier rejection, and JIT decline.
For each rejection, the red output from the unfixed tree is recorded before the
fix. The phase cannot close on a parser-only test: constructors, `?`, every method,
match branches, and a void function must execute.

## chantier 2 hostile review of round 2

### assertion 0

Compile-time match exhaustivity, must-use `Result` and `Option`, no surface `null`,
and checked `?` conversion are non-negotiable.

Round 2 is still not PROVEN. The review found seven implementation contracts that
were stated but not made exact enough: binding consumption in `let`, `Never` and
typed return checking, enforcement of the untyped-native boundary, allocation root
ordering for sum payloads, the complete JIT decline set, and the distinction between
design proof and implementation evidence. These are revised below.

## chantier 2 design closure revisions

The review findings are accepted. They are implementation proof obligations, not
reasons to weaken the design gate. This section makes each obligation a concrete
static or runtime rule before the final design review.

### bindings and must-use

`let name = result` and `let mut name = result` are uses: the value is retained in a
named local and is available for a later return, match, method call, or explicit
discard. They are distinct from `let _ = result`, whose underscore is not entered in
the environment and is the only implicit drop spelling. `let _name = result` is a
real binding and therefore a use. A typed annotation on the binding is checked
before this distinction, so `let x: int = Result<int, string>` is a named type
mismatch rather than an ignored-sum case.

The must-use pass visits every initializer, assignment, return, call argument,
match scrutinee, match arm value, and expression statement with that context. It
does not infer use from whether a named local is later read. A later unused-local
warning is separate and cannot downgrade `IgnoredResult` or `IgnoredOption`.

### never and total returns

`Never` is a control-flow type, not a value. `return expr` checks `expr` against the
current function return type, then produces `Never`; `return` without an expression
is legal only for a `Unit` function and also produces `Never`. `break` and `continue`
are legal only in a loop and produce `Never`. `Never` unifies with the expected type
of a branch without manufacturing a register value.

The function control-flow pass computes whether each body path returns a value or
diverges. A declared non-Unit return rejects any reachable fallthrough with
`MissingReturnValue`; a declared Unit return may fall through to `Unit`. An
implicit-return `if`, `match`, or block is checked branch by branch. An arm or branch
ending in `return`, `break`, or `continue` contributes no value to type unification.
There is no implicit null return path in either typed or untyped compilation.

### FFI provenance and diagnostics

The symbol environment distinguishes `Dynamic` from `UntypedNative(name)`. Standard
module exports enter through a signature table and never use that marker. A custom
native export without a declared signature gets `UntypedNative` and can be called at
the ordinary dynamic boundary, but the following operations reject it with the
named `UntypedSumValue` diagnostic: `?`, a known sum method, a non-wildcard variant
pattern, or a sum-typed binding or return that would require proving its family. The
diagnostic names the export and asks for a typed wrapper or a declared signature.

The typed native signature table is passed through the compiler pipeline rather than
reconstructed from a runtime return value. Its entries include family, type
parameters, unit status, and whether absence or failure is represented by a sum.
This is the enforcement point for the standard-library null rule.

### GC allocation order

The sum constructor compiler always places its payload in a live VM register before
emitting `MakeOptionSome`, `MakeResultOk`, `MakeResultErr`, or `MakeErrorMessage`.
The interpreter constructor reads only that register. `VM::alloc_sum` allocates
through the ordinary heap path, whose collection root walk includes all registers
in the current frame, host roots, globals, and open upvalues before it installs the
new object. It publishes the new object only after allocation succeeds. A native
constructor receives the payload in its argument registers under the same rule.

The proof test allocates a string payload, constructs a sum, forces a collection,
then extracts and compares the payload. A second test constructs a sum immediately
before a threshold-triggering allocation. Both assert the extracted pointer is
valid and the result tag is unchanged. The heap children walker and size estimator
are covered by a direct `SumObject` test as well as an executing Aelys program.

### complete JIT decline set

The unsupported set is the complete sum opcode set, not a subset: `LoadUnit`,
`LoadNone`, `MakeOptionSome`, `MakeResultOk`, `MakeResultErr`, `MakeErrorMessage`,
`SumTest`, `SumPayload`, and the defensive sum-tag trap if it has an opcode. The
translator returns `None` for any one of these and for every wide form, before
publishing a machine-code entry. The root and OSR paths then execute the verified
interpreter. A test builds one function containing every opcode, requests JIT
translation, and asserts no machine-code entry is installed; an interpreter test
executes the same function and asserts the result.

The bytecode verifier recognizes every new opcode and checks register, family,
variant, payload, and jump operands. An unknown opcode still rejects the function.
The assembler, disassembler, binary serializer, and wide decoder use one shared
opcode table so a recognized opcode cannot be accepted by one path and misdecoded
by another.

### proof boundary

The design gate proves that the compiler and VM have an unambiguous, closed route
for every required rule. It does not claim that implementation tests already pass.
Those tests are deliberately written after the gate, run once against the unfixed
tree to capture RED output, and only then guarded by the implementation. The ledger
will record the red output and execution evidence during implementation. No design
claim below can be marked complete from a green test that was never made to fail.

## chantier 2 design gate close

The final hostile reviewer opened with Assertion 0 and returned `PROVEN`.

- Match exhaustivity uses recursive finite-domain coverage, excludes guarded arms,
  and traps invalid runtime tags.
- `Result` and `Option` must-use is enforced across initializers, statements,
  branches, returns, and bindings.
- The parser rejects surface `null`; source inference and emission use `Unit`.
- `?` uses the closed residual conversion table and rejects dynamic or unresolved
  cases.
- Untyped native exports cannot participate in sum operations.
- Verifier rules, GC roots, allocation ordering, and the complete JIT decline set
  have explicit enforcement points.

The design gate is terminal. Implementation may begin. Its RED outputs and execution
evidence remain mandatory and are not implied by this design decision.

## chantier 2 compile-fail RED evidence

The rejection tests were added before any error-handling fix and run against the
unfixed compiler. The command was `cargo test -p aelys --test
error_handling_tests`; it failed all four tests, proving the tests were live.

- `Result<int, string>` was rejected as `error[E0101]: expected >, found ,`, proving
  that multiple generic arguments were not accepted.
- `read()?` was accepted only far enough to reach `error[E0002]: invalid character
  '?'`, proving that the operator had no lexer support.
- `null` compiled successfully, so the helper panicked with `the source must be
  rejected`, proving the surface null hole was real.
- The missing-variant case also stopped at the one-argument type parser and did not
  produce a match diagnostic.

The test source remains in `aelys/tests/error_handling_tests.rs`; the RED run is the
required pre-fix observation, not a claimed implementation result.

The added `Err`, `None`, and string-to-`Error` cases were then run after temporarily
disabling their `SumTest` branches. The command reported three failures: two reached
`InvalidBytecode("temporary sum test baseline")`, and the conversion case exposed a
separate exhaustivity defect, `non-exhaustive match; missing Err(_)`, because nested
`Error::Message` coverage was not implemented. The temporary change was removed
immediately; this is the RED evidence for those tests.

## chantier 2 execution RED evidence

The first execution tests were added in `aelys/tests/error_handling_runtime_tests.rs`
and run before the execution backend was complete. `cargo test -p aelys --test
error_handling_runtime_tests -- --nocapture` failed both tests: the constructor and
match program reached `TypeInferenceError("sum expressions are not supported in the
VM backend")`, while the `?` program first exposed that the test needed an explicit
statement terminator after the operator on the same line. The tests therefore
exercised the compiler and were not green by accident.

The restored implementation then passed all five execution cases in the same suite:
`Ok` extraction, `?` propagation of `Err`, `None` propagation, string-to-`Error`
conversion wrapped back into `Result::Err`, and their corresponding matches. The
generated code uses the new sum opcodes and the invalid-match path is an explicit
VM trap.

Before adding the sum opcodes, the real `stage1.aelys` measurement was
`avbc=16125`, `instructions=849`, `allocations=76`, from
`cargo test -p raizen-engine --lib measure_stage_one_baseline -- --nocapture`.
The temporary measurement test was removed after the run; a matching post-change
measurement remains required before chantier 2 closes.

The must-use additions were checked by temporarily bypassing
`validate_must_use_values` and rerunning `cargo test -p aelys --test
error_handling_tests -- --nocapture`: both `ignored_result_has_a_named_diagnostic`
and `ignored_option_has_a_named_diagnostic` then failed because compilation
succeeded, while the explicit underscore case still compiled. The validation pass
was restored immediately. The normal suite now passes all six tests.

The combinator execution tests were written before lowering was enabled. A temporary
`return Ok(false)` in the sum-method compiler caused all four new cases to fail with
the named method being resolved as an undefined global (`unwrap_or_else`,
`unwrap_or`, `expect`, and `unwrap` respectively); the original five execution
tests stayed green. The temporary return was removed before implementation resumed.

The failure-branch combinator test was also run with the lowering temporarily
disabled. It failed at compilation with `UndefinedVariable("unwrap_or_else")`,
while the restored lowering executes the `None`, `Err`, lazy fallback, and closure
branches. The temporary change was removed immediately.

The unit-return execution test initially failed with `expected unit, got null` for
`fn noop() -> unit {}; noop()`. Replacing typed `Return0` and typed empty-branch
fallbacks with `Unit` made it pass; the RED run proved the old null return path was
reachable from a surface function even though the null literal itself was rejected.

### chantier 2 hostile implementation review 1

The senior review opened with Assertion 0 and returned `NON PROVEN`. Elyra and the
consumer were green, but the review found concrete type mismatches downgraded to
dynamic recovery, a `?` success type not linked to its source payload, untyped native
values satisfying annotations, unchecked generic arity, non-value match arms,
incomplete alternation coverage and binding registers, a discard binding for `_`,
legacy native null normalization, and non-isolated RED evidence. It also rejected
the existing GC, wide verifier, JIT and post-change measurement evidence as too weak.

### chantier 2 revision RED evidence

The following tests were added before the corresponding corrections and were run
against the unfixed implementation with `cargo test -p aelys --test
error_handling_tests -- --nocapture`:

- nine new guards failed: concrete return mismatch, non-boolean guard, incompatible
  match arms, `?` success mismatch, native annotation, generic arity, empty match arm,
  nested coverage, and underscore binding;
- before the recursive coverage fix, `Some(true) | Some(false)` reported missing
  `Some(_)`;
- before the shared binding-register fix, removing reservation and assignment made
  the runtime test fail with `undefined variable: number`;
- before the isolated exhaustivity fix, disabling only the check made the test fail
  with `the source must be rejected`, without an unused-result error.

The implementation now makes every concrete inference error fatal while retaining
`Dynamic` as an explicit boundary, checks generic arity, rejects untyped native
values at typed boundaries, links `?` payloads and residual conversion, requires a
value in every non-diverging match arm, recursively unions all alternation coverage,
does not bind `_`, and assigns every valid alternation binding through one register.
The legacy native boundary rejects `Value::null` with `NativeReturnedNull`; standard
absence still arrives as `Option::None`, and surface fallback branches emit `Unit`.

The corrected rejection suite has 21 passing tests. The corrected runtime suite has
16 passing tests, including qualified constructors, nested guarded alternatives,
match block tails, and selected-alternative bindings. The native null rejection test
passes independently.

### chantier 2 hostile design review 2

Rawls opened with Assertion 0 and returned `NON PROVEN` after an independent run.
The rejection suite was 21/21 and the execution suite 16/16, but concrete equality,
indexing, iteration, collection assignment, and field mismatches were still accepted;
`UntypedNative` still unified with every type and could cross sum combinators; several
annotation paths bypassed generic arity checks; and the GC, JIT, and Stage 1
measurement evidence lacked the requested end-to-end or reproducible harnesses. The
review also observed six consumer failures. The error chantier remains open until
these findings are corrected and both repositories are rerun.

### chantier 2 revision round 3

The compiler-boundary tests were extended to cover explicit `dynamic`, native values
at equality, guards and sum methods, scalar indexing, non-iterables, vector element
assignment, a provably bad literal index, struct fields, `Result<int, bool>` error
conversion, and generic arity in function parameters, returns, casts and struct
fields. The corrected rejection suite is now 35/35.

The required RED observations were executed after each test was live: removing the
literal-index guard produced `the source must be rejected`; removing the explicit
`dynamic` exception produced `untyped native 'custom::read' cannot satisfy annotation
dynamic`; restoring the old untyped-native wildcard made the equality test panic at
`an untyped native must not enter a sum operation`; removing scalar indexing produced
`the source must be rejected`; removing vector element constraints produced the same
panic; and removing struct field lookup produced `the source must be rejected by type
inference`. Removing only the non-iterable diagnostic reached the backend with
`for-each over Dynamic not yet supported`, proving the test was live but also exposing
that the diagnostic must remain in inference. All guards were restored.

The native boundary was rebuilt instead of weakened: ABI version 4 exports a
validated primitive signature for macro-generated native functions, the runtime
passes those signatures into sema, and unannotated exports remain explicitly
untyped. The registered-native test was green at first; removing signature transfer
made it fail with `the native signature must reject a bool argument`, then the
transfer was restored. The existing ABI fixtures were updated with an absent
signature, preserving their intentional untyped status.

The JIT gate was strengthened to cover all six sum opcodes in compact and wide
forms, all five translation entry points, and the actual provider cache path. With
`contains_sum_opcode` temporarily replaced by `false && ...`, the first translation
assertion failed at `translate_integer_function(&function).is_none()`, proving the
provider test would have entered the JIT without the guard. The predicate was
restored and the 12 opcode/form cases passed.

An end-to-end bytecode test now invokes a native payload producer, forces an
allocation safepoint while the payload is in a VM register, executes `MakeSum`,
and verifies the returned sum and string after a major collection. Removing the
allocator's payload root made the focused direct regression fail with
`assertion failed: vm.heap().get(payload).is_some()`; the bytecode test itself
continued to pass because the register is already a VM root, so the direct test
remains the proof for the allocator's host-root obligation. The root was restored.

### chantier 2 revision round 4: consumer closure and cast boundary

The first complete consumer run after the type-boundary corrections rejected the
real `stage1.aelys` module because globals declared after `main` were not visible
to the function body. A top-level global-binding prepass now records declared
types before function inference. The consumer then compiled the stage and passed
the exact full run with 146 passed and 0 failed.

That run exposed a separate unsound acceptance: `as float` was accepted by sema
but the typed backend emitted only its operand. `AddFF` then read the integer
returned by `raizen::frame_count` as a float and hit
`bytecode/src/value/accessors.rs:55:9: type confusion: not a float` in both
`native::tests::getters_read_the_current_frame_snapshot` and
`native::tests::enemy_death_exposes_id_and_position_once`. This is the required
RED evidence for the cast boundary; the focused tests were red before the
correction.

The backend now emits one checked `Cast` opcode with an explicit primitive target
for integer, float, and boolean conversions. Compact and wide dispatch, bytecode
verification, register liveness, assembly, disassembly, and the runtime type and
overflow errors all cover it. The two focused consumer tests pass, the elyra
workspace passes `cargo test --workspace --quiet`, and the cast test suite passes
9/9. The existing test assertions were not weakened; the consumer catches the
previously invisible execution defect.

The required post-change measurement used the same temporary harness as the
baseline and was removed afterwards. On the real `raizen-scripts/stage1.aelys` it
reported `avbc=16125`, `instructions=849`, `allocations=76`, exactly matching the
pre-sum baseline. The added error representation does not change this script's
bytecode or top-level dispatch path, so no AVBC or value-representation rebuild
was justified by the measurement.

### chantier 2 hostile design review 3: open findings

Locke opened with Assertion 0, made no edits, and independently ran the elyra
workspace, the consumer workspace, and `cargo xtask ci`. All three were green;
the consumer count was 146 passed and 0 failed. The phase nevertheless remains
NON PROVEN because the review found four violations of the language bar: an
implicit tail in an explicit `unit` function bypassed must-use checking, known
sum methods were reachable through an explicit `dynamic`, empty vector pop and
safe collection get still exposed `null`, and `unwrap`/`expect` failed as
`InvalidBytecode` without preserving the expect message.

The new regression tests were run before their fixes. Their exact RED evidence
was:

- `implicit_unit_tail_cannot_discard_a_result`: the test panicked at `the source
  must be rejected`.
- `dynamic_values_cannot_use_sum_methods_without_a_sum_type`: compilation
  returned `error[E0201]: undefined variable 'unwrap'` instead of the named
  dynamic-sum diagnostic.
- `empty_vec_pop_returns_option_none`: `expected Option::None, got null`.
- `unwrap_and_expect_fail_with_structured_messages`: `error: invalid bytecode:
  match reached an invalid runtime value`.

The review also identified that public inference adapters still collapsed named
semantic errors into `E0301`, so the correction must carry the specific error
code through the compiler API. No collection or struct phase may open until a
fresh review accepts these repairs.

### chantier 2 revision round 5: hostile findings repaired

The implicit-return path now records a must-use residual when the declared
return type is \`unit\`, so a result-valued tail cannot be discarded. Explicit
\`dynamic\` values reject the complete sum-method surface with a dedicated
diagnostic. Collection pop and safe-get operations now produce
\`Option::Some\` or \`Option::None\`; no collection failure path constructs the
forbidden null sentinel. \`unwrap\` and \`expect\` carry a family code and, for
\`expect\`, the message register through compact and wide \`MatchFail\` forms to
a structured \`SumUnwrapFailed\` runtime error.

The public adapters now preserve semantic diagnostic codes instead of wrapping
all inference errors in \`E0301\`. A new regression first passed with
\`error[E0302]\`; after temporarily replacing the adapter with the old generic
mapping, it failed exactly with \`error[E0301]: type error: non-exhaustive match;
missing Err(_)\`. The named adapter was restored.

Changing \`Vec::pop()\` to return an Option made seven old raw-value collection
tests fail. Their exact failures were compile-time \`unused Option value\` for
the discarded tails and \`type mismatch: expected Option<i64>, found i64\` or
\`type Option<i64> is not one of [...]\` for arithmetic. The tests now consume
the values with \`unwrap\`, and the wide bytecode tests extract the Some payload
explicitly. The resulting array suite is 99/99 and the wide boundary suite is
35/35. The focused error rejection suite is 38/38 and the error execution
suite is 18/18.

### chantier 2 revision round 6: standard-library type boundary

The hostile review's remaining concrete type hole was turned into
`standard_native_signature_rejects_wrong_math_argument`. Before changing the
implementation, `cargo test -p aelys --test error_handling_tests
standard_native_signature_rejects_wrong_math_argument -- --nocapture` went red:
the helper panicked with `the source must be rejected`, because `math::sqrt` still
advertised a `dynamic` parameter.

The boundary was rebuilt with an internal numeric constraint and concrete
primitive signatures for the standard modules. `math::sqrt("bad")` now fails at
inference while integer and floating-point math calls remain accepted. The
standard math suite is 60/60, the elyra workspace is green, and the consumer's
exact full run is again 146 passed and 0 failed.

The independent hostile review that preceded this correction reported one
consumer failure in `bundled_stage_loader_emits_original_asset_paths` at
`bridge.rs:752` after 118 passes. The same exact consumer command passed
immediately afterwards and then passed five consecutive full runs; the focused
test passed 12 consecutive runs. No source change was made for that
non-reproduction, and it remains recorded as a review flake rather than
accepted as green evidence.

The same audit found that an explicit `dynamic` value could still satisfy a
concrete annotation. The new `dynamic_value_cannot_satisfy_a_concrete_annotation`
guard was run before its correction; the helper went red with `the source must
be rejected` because `let number: int = value` was accepted.

Concrete annotations now reject explicit `dynamic` values at let bindings,
typed calls, assignments, explicit returns, and implicit non-unit tails. The
guard passes and the rejection suite is 40/40. The dynamic escape remains
available when the expected type is explicitly `dynamic`.

The first full-suite rerun after that boundary change exposed two legitimate
over-rejections. An unannotated function returning `arr[1]` was treated as if
its still-free return variable were concrete; `array_tests` went 97/99 with
the exact mismatch `expected τ3, found dynamic (return type of function
'get_element')`. Allowing a free inference variable to absorb `dynamic` while
keeping concrete annotations closed restored 99/99.

The same rerun exposed a missing typed method boundary: `error.len()` inside a
function returning `int` was still inferred as `dynamic`, producing the exact
failure `expected i64, found dynamic (return type of function 'length')`.
String method signatures now flow through sema, and the runtime error suite is
18/18.

Imported Aelys functions previously lost their declared signatures at the
module boundary. The existing global-sync test caught this as
`expected Option<i64>, found dynamic (type annotation on variable 'before')`.
Script export signatures now accompany module loading, and the focused test
passes. Numeric-preserving math signatures use the internal `number` type so
`sign(sqrt(-1.0))` remains typeable; `stdlib_math_tests` is 60/60.

At this checkpoint the consumer's exact `cargo test --workspace --quiet` run is
146 passed and 0 failed. The hostile review remains open; no collection phase
has been opened.

The collection syntax inventory is now explicit. The parser currently accepts
literal `[1, 2, 3]`, sized `[; n]`, typed `Array<T>[...]`, typed `Vec<T>[...]`,
untyped `Array[...]` and `Vec[...]`, empty `Array<T>[]` and `Vec<T>[]`, sized
`Array<T>(n)` and `Array(n)`, and the older `Array[; n]` forms. It also accepts
indexing, range slices in the AST, `for item in collection`, and the existing
`push`, `pop`, `len`, `capacity`, `get`, `reserve` subset. It does not yet
provide `vec![...]`, `iter`, `map`, `filter`, `fold`, or reference iteration
with `for item in &collection`.

The seven consumer scripts contain the old typed vector spellings only in
`stage1.aelys`: `Vec<String>[...]`, `Vec<Int>[...]`, `Vec<Float>[...]`, and
empty `Vec<Int>[]`, plus indexing and mutation. The other six scripts contain
no collection literal spelling. The migration break will therefore be named
and covered before the old constructors are removed. The current design choice
is distinct fixed `Array<T>` and growable `Vec<T>` types; the new surface will
use `[... ]` for fixed arrays and `vec![...]` for vectors, with typed
annotations retained where inference cannot determine an empty collection.

An additional hostile boundary probe covered nested values. Before the fix,
`read().unwrap_or(fallback)` with `fallback: dynamic` was accepted and the
new test failed at `the source must be rejected`. A second probe with
`Some(dynamic)` was rejected only as `error[E0307]: cannot infer the sum type
for Some`, which was safe but did not identify the boundary. Dynamic rejection
now recurses through `Option`, `Result`, arrays, vectors, tuples, and function
types; sum combinator arguments use the same check. The two probes now produce
the named type-mismatch diagnostic, and the existing E0307 remains reserved
for genuinely unresolved constructors.

The fourth hostile review returned NON PROVEN with two fresh runtime-boundary
findings. The required RED probes were:

- `fn f(value: int) -> dynamic { "bad" }; Some(1).map(f).unwrap() + 1`,
  which compiled and then reported `type confusion: not an int` followed by
  `runtime panic recovered`;
- `fn f() -> int { math::abs(-1.2) }; f() + 1`, which compiled and then
  reported the same integer accessor confusion.

The callback boundary now rejects a dynamic return when a combinator expects a
concrete mapped value. Numeric standard-library signatures are specialized per
call to one fresh numeric variable constrained to the concrete numeric set;
the selected argument type therefore propagates to the return. The two new
compile-fail guards pass, the math suite remains 60/60, and the complete error
rejection suite is 44/44. The next hostile review must rerun both RED probes.

### chantier 2 revision round 7: residual dynamic boundaries and numeric abi

The next four compile-fail guards were executed before their corrections. The
complete error rejection file then reported 48 passed and 4 failed, each helper
stopping at `the source must be rejected`: a `dynamic` index, a `dynamic` sized
array length, a `dynamic` match guard, and a `dynamic` argument to
`Error::Message`. Index reads and writes, sized construction, guards, and the
error constructor now use the same concrete-boundary rejection path. The file is
52/52.

The runtime math boundary had two independent RED observations. Before the
runtime correction, `sign_float_preserves_type` returned `None` from
`Value::as_float()` because `sign(-1.0)` returned an integer. After the correction,
the existing `pow_int_small_exp` test went RED with `Expected int 1024 but got
1024.0`, proving that the old integer fast path contradicted the declared `f64`
signature. Floating `sign` results now remain floating values, and `pow` always
returns a float, including values outside the NaN-boxed integer range. The migrated
math suite is 61/61 and the edge-case suite is 54/54; the consumer exact run is
146 passed and 0 failed.

The full elyra workspace is green after this round, including 99/99 array tests,
39/39 iteration and string-method tests, 52/52 error rejection tests, 61/61 math
tests, and 18/18 error execution tests. `cargo xtask ci` is green. The phase is
still open pending the mandated fresh hostile review, which must independently
rerun the compile-fail probes, runtime probes, both repository suites, and assess
the complete language bar before any collection implementation begins.

### chantier 2 hostile review 5: dynamic control-flow boundary

Dirac opened with Assertion 0 and independently ran `cargo xtask ci`, the complete
consumer workspace, the error rejection and runtime suites, and the requested
numeric and callback probes. The consumer count was 146 passed and 0 failed. The
review remained NON PROVEN because explicit `dynamic` was silently accepted as a
boolean condition and reached the VM:

`let value: dynamic = "bad"; if value { 1 } else { 0 }` executed and returned
`1`. The reviewer also identified the same missing static boundary for ordering
comparison; `let value: dynamic = "bad"; value > 0` reached a runtime numeric type
error instead of being rejected by inference.

The two new rejection tests were run before the correction. The error test file
reported 52 passed and 2 failed, both at `the source must be rejected`. Conditions
in expression and statement `if`, `while`, and numeric `for` bounds now reject
dynamic and untyped native values before adding equality constraints. Logical
operands, range bounds, ordering comparisons, and bitwise operands use the same
concrete boundary. The rejection suite is 54/54.

The phase remains NON PROVEN until another fresh hostile review independently
confirms these fixes and the complete Stage 1 error bar. No collection work has
opened.

The first full-workspace rerun after closing this boundary exposed an
over-rejection in previously valid untyped collection functions. The array suite
went 97/99: `a.len()` and `v.len()` in functions with inferred collection
parameters became dynamic and were then rejected by the new numeric comparison
check. Collection method inference now constrains a free receiver variable to
`Array<T>` or `Vec<T>` for `len` and `get`, preserving the existing valid programs
without weakening explicit `dynamic` rejection. The focused array suite is back
to 99/99. A fresh review is still required against this corrected tree.

That receiver constraint was itself too strong for an inferred scalar later used
with a string method: the full workspace exposed `security_audit_tests` at
`string_concatenation` with `type string is not one of [Array(...), Vec(...)]`.
The free `len/get` receiver path now leaves the receiver unconstrained and only
supplies the method result type; concrete substitution still selects the typed
string or collection backend, while untyped functions retain their existing
runtime boundary. The string probe and array suite pass again.

### chantier 2 revision round 8: corrected tree verification

The four direct CLI probes were rerun after the control-flow fixes. Explicit
`dynamic` in an `if` condition is rejected as `E0301 expected bool, found dynamic
(if condition)`. Ordering comparison is rejected as `E0301 expected number, found
dynamic (comparison)`. The corresponding `while` probe is rejected as
`E0301 expected bool, found dynamic (while condition)`, and the numeric `for`
probe is rejected as `E0301 for-each over Dynamic not yet supported`.

The first rerun of `cargo xtask ci` found two Clippy `collapsible_if` failures in
range-bound inference. Collapsing those branches preserved the concrete-boundary
checks. The corrected rerun completed format checking, workspace Clippy with
`-D warnings`, workspace tests with all features, and `git diff --check` with
exit status 0. The hostile review is still pending; the error-handling phase is
not closed and collections remain unopened.

### chantier 2 hostile review 8: residual dynamic receivers

The fresh review found three additional boundaries. Before their corrections,
the new rejection guards produced exactly 54 passed and 3 failed in
`error_handling_tests`: a dynamic condition in an implicit return was accepted,
`value: dynamic` could be indexed and reached the VM, and
`fn length(value) -> int { value.len() }` was not rejected with a collection
diagnostic. The last source reached an undefined-member backend path rather than
teaching the programmer which receiver types were valid.

The implicit-return `if` now rejects both `dynamic` and untyped native conditions.
Indexing a dynamic receiver emits the named invalid-index error. Free receivers
used by collection methods now carry a `string`, `Array<T>`, or `Vec<T>` choice
for `len`, and an `Array<T>` or `Vec<T>` choice for `get`; a concrete scalar call
therefore fails during inference. Explicit dynamic collection receivers emit a
named collection-method diagnostic. The corrected rejection suite is 57/57.

The review's Clippy observation was independently cleared by the preceding
round: the corrected `cargo xtask ci` completed with exit status 0. A new fresh
hostile review is required after these receiver fixes.

### chantier 2 revision round 9: dynamic iteration and indexed writes

The next guards were executed before their corrections. The error suite reported
57 passed and 3 failed out of 60: a dynamic for-each had an unhelpful backend
message, while indexed assignment through a dynamic receiver and through a free
receiver called with an integer were accepted. Dynamic for-each now emits the
sema `NotIterable` diagnostic, dynamic indexed assignment emits `cannot index`,
and free indexed-write receivers are constrained to `Array<T>` or `Vec<T>` with
the assigned element checked against `T`. The corrected suite is 60/60.

The post-fix `cargo xtask ci` completed format, Clippy with `-D warnings`, all
workspace tests with all features, and `git diff --check` successfully. The
post-fix consumer run completed with 146 passed and 0 failed. The phase remains
open until a fresh hostile review audits this exact tree and calls the design
PROVEN.

### chantier 2 hostile review 9: inferred unary boolean boundary

Planck's independent review found one remaining silent acceptance. Before the
correction, the exact probe
`fn invert(value) -> bool { not value }; invert(1)` executed and returned
`false`; the unary `not` path rejected dynamic and untyped native values but did
not constrain an inferred operand to `bool`. The new guard was run before the
fix and reported 60 passed and 1 failed in `error_handling_tests`.

Unary `not` now adds an equality constraint to `bool` when its operand is still
inferred. The corrected CLI probe emits `E0301 type mismatch: expected bool,
found i64 (function call)`, and the rejection suite is 61/61. The phase remains
open pending another fresh hostile review of the exact tree.

### chantier 2 revision round 10: member receiver boundaries

Direct probes found two runtime paths not covered by the previous guards:
`value: dynamic; value.to_upper()` returned an object instead of a diagnostic,
and `fn push(value) { value.push(1) }; push(1)` recursed until stack overflow.
An inferred string method on an integer was rejected only as an undefined member.
The three new guards were run before the fix and reported 61 passed and 3 failed
out of 64.

String method calls now require a `string` receiver, including free receivers
and explicit `dynamic`; collection methods constrain free receivers across
`len`, `get`, `push`, `pop`, `capacity`, and `reserve`, and reject invalid
concrete receivers before backend dispatch. The corrected rejection suite is
64/64. The CLI probes now emit named E0311/E0312 diagnostics and no longer reach
the VM. A fresh hostile review is still required.

The post-fix `cargo xtask ci` completed successfully, including format, Clippy
with `-D warnings`, all workspace tests with all features, and `git diff --check`.
The post-fix consumer suite completed with 146 passed and 0 failed. No phase
closure is claimed until a fresh reviewer independently reruns the direct probes
and calls the design PROVEN.

### chantier 2 hostile review 11 and revision RED evidence

The fresh hostile review found one remaining dynamic arithmetic boundary. The
direct source `let value: dynamic = "bad"; value + 1` compiled and reached the VM,
which reported `type error in 'addition'`. The reviewer marked the phase NON PROVEN
because this operation can be rejected before execution; the recorded design's
explicit dynamic boundary does not override the Stage 1 compiler-first bar.

The new compile-fail guard `dynamic_arithmetic_is_rejected_before_execution` was
run before the correction with `cargo test -p aelys --test error_handling_tests
dynamic_arithmetic_is_rejected_before_execution -- --nocapture`. It failed at
`the source must be rejected`, with exit status 101 and zero passing tests. The
guard is live and the implementation correction is still pending.

The arithmetic boundary now rejects `dynamic` and untyped-native operands before
constraints are emitted, using the existing binary-operator diagnostic. The
corrected rejection suite is 65/65, the execution suite remains 18/18, and the
math suite remains 61/61. A fresh exact-model hostile review is still required;
this correction does not close the chantier by itself.

The first `cargo xtask ci` after this guard was intentionally run before declaring
the correction complete. It exposed one over-rejection in the existing API suite:
`test_call_function_with_globals` failed while compiling `counter += n` because a
global created by an earlier REPL input was reintroduced as `dynamic`. The focused
test was red with the named arithmetic diagnostic, not a runtime failure.

The first strict correction was too broad at the dynamic boundary: treating every
`Dynamic` value as explicit made separately compiled REPL globals and imported
functions fail in existing module and API tests. That correction was removed. The
delivered rule tracks explicit-dynamic provenance through annotations, parameters,
annotated returns, and copies; unknown host or imported boundaries remain dynamic,
while an explicitly dynamic value cannot cross a typed arithmetic boundary. The
global-call test is green without a runtime signature registry.

The next hostile review also found that `fn read() -> Result<int, string> { Ok(1) }
let value = read(); 0` compiled and silently dropped the named binding. The new
guard `unused_result_binding_has_a_named_diagnostic` was run before the dataflow
fix with `cargo test -p aelys --test error_handling_tests
unused_result_binding_has_a_named_diagnostic -- --nocapture`; it failed at `the
source must be rejected`, exit status 101, with zero passing tests. The guard is
live and the binding-use pass is still pending.

The same review found that the constant source `"ab"[2]` compiled and reached the
VM, which reported a runtime index error. The new guard
`constant_string_index_out_of_bounds_is_rejected` was run before its correction
with `cargo test -p aelys --test error_handling_tests
constant_string_index_out_of_bounds_is_rejected -- --nocapture`; it failed at
`the source must be rejected`, exit status 101, with zero passing tests.

The reviewer also ran the inferred scalar collection probe and found an internal
type variable in its public diagnostic: `type i64 is not one of
[Vec(Var(TypeVarId(2)))]`. The new guard `collection_diagnostics_hide_inference_variables`
was run before the formatting correction and failed with that exact message,
exit status 101. This RED output proves the diagnostic assertion is live.

Collection receiver diagnostics now render the method-specific requirement:
`collection method 'push' requires a vector, found i64` and the dynamic form uses
the same wording without exposing `TypeVarId`. The diagnostic guard is green, and
the generic type display uses `inferred type` rather than an internal variable id.

### chantier 2 revision round 12: must-use migration and verification

The lexical binding pass deliberately superseded the earlier design note that
treated every named binding as a use. A named `Result` or `Option` binding now
requires a later lexical read, return, match, method call, or another consuming
context; `let _ = value` remains the sole explicit discard. This closes the silent
drop found by the hostile review without turning ordinary scalar locals into a
warning system. Lambda parameters are entered into the nested analyzer scope so
their sum types cannot be falsely reported as unused.

The new must-use rule first exposed eight pre-existing standard-library tests that
intentionally ignored `Option` values. The FS EOF test was migrated to three
explicit `let _ = fs::read_line(f)` statements. The SYS tests now explicitly discard
the unused results of `sys::arg`, `sys::env`, `sys::home`, `sys::script_path`, and
`sys::script_dir`. The focused SYS suite is 27/27 and the FS suite is 16/16.

The corrected tree then completed `cargo xtask ci` with exit status 0, including
format, Clippy with `-D warnings`, all-feature workspace tests, and `git diff
--check`. The rejection suite is 68/68, the runtime error suite 18/18, and the
consumer rerun completed the exact grouped total of 146 passed and 0 failed. A
single earlier consumer run had one timing-sensitive event test failure; its
isolated rerun passed and the subsequent full consumer suite passed 25, 94, 4, 8,
7, and 8 tests respectively.

The language specification still described the removed null surface, gradual
runtime typing, and dot-separated module calls. Those claims were corrected in
docs/language-spec.md: absence and failure now use Option and Result, unit is
the no-value type, module and associated paths use ::, and the error section
documents exhaustive matching, ?, and must-use handling. docs/aelys_issues.md
is not present in elyra; the prior-art copy in the consumer was read and remains
consumer-owned rather than being silently copied into this repository.

The first `cargo xtask ci-full` attempt after the revision was intentionally
recorded before closure. It stopped at `cargo fmt --all -- --check` with exit
status 1 because the new `CollectionMethodReceiver` diagnostic variant and
nearby `ConstraintReason` fields were not rustfmt-shaped. Running `cargo fmt
--all` corrected only that mechanical formatting issue; the full command must
still be rerun to establish the terminal result.

### chantier 2 hostile review 13: NON PROVEN findings

The fresh hostile reviewer was dispatched with the required `gpt-5.6-luna` and
`max` request and opened with Assertion 0. It explicitly declined to claim that
the interface had verified those model settings, so this record does not treat
it as exact-model execution. Its verdict was NON PROVEN, with the following
current-tree findings ordered by risk:

- explicit `dynamic` still reaches unary numeric negation, bitwise negation, and
  equality; the compiler accepts `let value: dynamic = "bad"; -value` and the VM
  reports a type error instead of sema rejecting it;
- generated foreign native wrappers encode declared unit returns as the legacy
  null value, which the runtime rejects as `NativeReturnedNull`;
- fallible fs APIs still advertise scalar return types and surface failures as VM
  errors, while `net::udp_connect` is declared unit and silently reports failure;
- `Result::unwrap_or_else` is inferred as a zero-argument callback although the
  backend invokes a Result fallback with its error payload;
- native type exports install a null sentinel as a global in both loaders;
- the consumer test references six of the seven Raizen scripts and has no
  standalone coverage for `bullet_patterns.aelys`.

The reviewer ran `cargo xtask ci` with exit status 0. It also reported one
earlier timing-sensitive consumer failure and a controlled rerun of the exact
consumer groups at 25, 94, 4, 8, 7, and 8 passed with zero failures. These
findings reopen the error chantier; no phase closure or commit is allowed until
each is corrected or explicitly owned by a later phase with a recorded reason.

### chantier 2 revision round 14: hostile guards RED

Three live guards were added before their fixes. `cargo test -p aelys --test
error_handling_tests dynamic_unary_numeric_operators_are_rejected_before_execution
-- --nocapture` exited 101 because the first dynamic unary source compiled and
the helper panicked at `the source must be rejected`. The equality guard was run
separately with the same command shape and exited 101 for the same reason.
`cargo test -p aelys --test error_handling_runtime_tests
result_unwrap_or_else_receives_the_error -- --nocapture` exited 101 with the
named compiler error `wrong number of arguments: expected 1, found 0`.
`cargo test -p aelys-native --test value_and_wrapper_tests
generated_unit_wrapper_returns_unit_not_null -- --nocapture` exited 101 after
the wrapper returned successfully but the output still satisfied `value_is_null`.
These RED results prove all four guards exercise the unfixed behavior.

### chantier 2 verification attempt: ci-full ASAN RED

The second full-gate attempt was run after cargo fmt --all had corrected the
first formatting failure. The debug and release workspace suites passed, and
Miri passed all five selected runtime tests in 1774.68 seconds. The ASAN
compile then failed in the in-progress filesystem Result migration: one
format string omitted its argument, RuntimeErrorKind was unused, and ten
map_or_else calls borrowed VM through two closures at once, producing E0524.
The command exited 1 before ASAN tests ran. The format string, import, and
closure shape were corrected mechanically; the full gate remains open until
the filesystem signatures, tests, and remaining fallible APIs are complete.

### chantier 2 revision round 15: fallible native boundaries

The filesystem operational API now returns Result values with string errors:
open, close, read, read_line, read_bytes, write, directory and file-management
operations, text helpers, size, readdir, join, and absolute. The read_line
contract is Result<Option<string>, string>; EOF is Option::None inside Ok.
Pure path predicates and basename, dirname, and extension remain infallible.
The old fs tests were migrated to unwrap or exhaustive match expressions.

The same rule was applied to network operations that had a Unit return while
silently discarding an OS failure: udp_connect, close, set_timeout, set_nodelay,
shutdown, and udp_set_broadcast now return Result<unit, string>. The UDP receive
registration arity was corrected from three to its actual two arguments. Native
Type exports no longer install a null global or enter the value export table;
Stage 1 has no runtime type registry, so those metadata exports remain available
to the loader but are intentionally not surface values. Exposing them as a
future type namespace is handed to Stage 2.

Changing fs signatures first exposed the old adversarial and security snippets
expecting VM errors. They were migrated to handle Err as a value; the focused
adversarial suite is 21 passed, 3 ignored and the security suite is 37 passed,
1 ignored. The network suite is 11 passed, 7 ignored. The full workspace gate
and consumer acceptance row remain open after this round.

### chantier 2 consumer coverage guard: bullet_patterns RED

A new Raizen bridge test was written for the previously uncovered
bullet_patterns.aelys script. Its first execution was RED: the script failed to
load with E0301 because its public pattern functions used bare return statements
without a unit return annotation, while their native emission path was inferred
as i64. The focused consumer command ran one test and exited 101 with the
diagnostic at bullet_patterns.aelys line 14. The script was corrected by
declaring unit returns on ring, spiral, line, fan, and wall; the same focused
test then passed. This is the required RED evidence for the new guard.

The first ci gate after that round stopped at the format check because the
empty native Type match arm had not yet been rustfmt-shaped. cargo fmt --all
fixed the mechanical branch layout; no compiled test had run in that attempt.

### chantier 2 revision round 16: native Option success RED

A new execution guard was added for a successful standard-native Option:
convert::parse_int("42") must match Some(42). Before the runtime correction,
the focused test exited 101 with InvalidBytecode saying that match reached an
invalid runtime value. The native returned a raw integer despite its declared
Option<int> signature. The guard is live; all successful Option native paths
must be wrapped before the phase can close.

### chantier 2 revision round 16: native Option and Result completion

The first run of the seven ignored network tests after the Option guard was
added was intentionally RED: five tests reached SumUnwrapFailed for a raw
Option value, and udp_set_timeout reached SumUnwrapFailed for a raw Result
value. udp_recv_negative_max also exposed that its old expression statement
was rejected by the new must-use rule before the runtime negative-bound check;
the test now explicitly discards that Option with `let _ =` so it reaches the
native diagnostic.

Every successful Option-producing native path in io, convert, sys, fs, and net
now allocates its declared Some value. net::set_timeout also applies its
fallible operation to UDP sockets, matching the existing ignored integration
coverage. The focused native Option guard passes, and
`cargo test -p aelys --test stdlib_net_tests -- --ignored --nocapture` passes
7/7 ignored tests with zero failures.

### chantier 2 hostile review attempts after round 16

Two fresh review dispatches were made with the only permitted configuration,
model `gpt-5.6-luna` and reasoning effort `max`, both opening with Assertion 0.
The service first returned a capacity error before running Hooke. A second
dispatch to Harvey returned `NON PROVEN` explicitly because that model was not
available or verifiable; it ran no repository command and performed no review.
Neither response is evidence about the tree, and no other model was used.
The error chantier therefore remains open until an exact-model hostile review
actually executes and returns a technical verdict.

A third exact-model dispatch, Jason, was attempted after the previous capacity
failure. It again returned `NON PROVEN` before repository inspection because
`gpt-5.6-luna` at `max` was unavailable or unverifiable in the service. No
substitute model, repository command, or review claim was accepted.

A fourth exact-model dispatch, Mill, returned the same preflight `NON PROVEN`
because `gpt-5.6-luna` with `max` was unavailable or unverifiable. It ran no
repository command. The phase has no executable exact-model hostile verdict.

### chantier 2 revision round 17: qualified sum value diagnostics

A compile-fail guard for `let value = Result::None` was added after the
local audit found that an invalid sum-family value path could fall through
to dynamic. Its unfixed run exited 101: the compiler rejected the source
only as `error[E0301]: undefined variable: Result`, not as a named unknown
variant. The guard therefore exercised the wrong diagnostic path.

The sema member resolver now recognizes the built-in sum namespaces before
resolving them as ordinary identifiers and emits `UnknownVariant` for invalid
members such as `Result::None`, while retaining constructor calls for valid
members. The focused guard passes with the named diagnostic. No surface
constructor or runtime representation was changed.

The complete rejection suite is now 71/71 and the execution suite is 20/20.
The local formatting and diff checks pass. The exact consumer row and the
full ci gate remain open after this source change.

The consumer acceptance row was rerun after the correction with the exact
`cargo test --workspace --quiet` command: 25, 94, 4, 8, 7, and 8 tests
passed, for 146 passed and zero failures.

### chantier 2 revision round 18: null introspection RED

A compile-fail guard was added for the remaining surface entry point
`convert::is_null(1)`. Before the removal, the focused command
`cargo test -p aelys --test error_handling_tests null_introspection_is_not_a_surface_api -- --nocapture`
exited 101 because the helper panicked with `the source must be rejected`.
The compiler therefore still accepted a function whose only purpose was to
expose the forbidden null representation. The guard remains live while the
runtime export, native signature, and diagnostic are removed.

The guard was then fixed by removing `convert::is_null` from the runtime
registry and native signature table and by making that path produce the
existing null replacement diagnostic. The full rejection suite passes 72/72
and the execution suite passes 20/20. The standard-library reference was
updated at the same time: module paths use `::`, absence is documented as
`Option`, fallible filesystem operations use `Result`, and no surface null
API remains in that reference.

The exact local `cargo xtask ci` gate then passed: formatting, Clippy with
`-D warnings`, the workspace all-features test suite, and `git diff --check`.
The older `cargo xtask ci-full` process is still running its Miri stage; its
result predates this round and will not be treated as evidence for the final
tree.

### chantier 2 revision round 19: explicit Error constructor RED

An execution guard was added for the public spelling used by the pattern
surface, `Error::Message("bad")`. The unfixed focused command
`cargo test -p aelys --test error_handling_runtime_tests error_message_constructor_executes -- --nocapture`
exited 101 with `undefined variable: Error` at the constructor. The pattern
matcher already recognized `Error::Message`, but value construction did not;
the guard stays live while the constructor paths are aligned.

The sema and backend constructor paths now use the same capitalized variant.
The focused guard passes, and the complete execution suite is now 21/21.

The exact consumer command `cargo test --workspace --quiet` was rerun after
this source correction and passed the locked groups 25, 94, 4, 8, 7, and 8:
146 passed and zero failures.

A fresh hostile-review dispatch to Curie used exactly `gpt-5.6-luna` with
reasoning `max` and opened with Assertion 0. The service could not verify that
model configuration, so Curie returned `NON PROVEN` before inspecting either
repository or running a command. No other model or substitute verdict was
used; the error chantier remains open.

The fresh `cargo xtask ci-full` reached the workspace tests but stopped at
71/72 in `error_handling_tests`: the existing dynamic Error-message guard still
used the obsolete lowercase `Error::message` spelling after the constructor
was made consistently `Error::Message`. Its exact output was the named
diagnostic `unknown variant 'Error::message' for Error`, so the failure was a
live stale guard rather than a runtime or compiler regression. The guard is
being aligned before rerunning the gate.

### chantier 2 revision round 20: deterministic Miri collection guard

After the stale constructor guard was fixed, a fresh `cargo xtask ci-full`
passed formatting, Clippy, debug tests, and release tests, then stalled in the
Miri runtime test `bytecode_make_sum_keeps_a_native_payload_across_collection`.
The test's `while !should_collect()` filler loop could cross the threshold in
the allocator's pre-allocation collection path without ever observing the
threshold, so the test was not a valid deterministic safepoint. The process
was interrupted with exit 130 after the live hang was confirmed.

The native safepoint now calls the VM's explicit `collect()` operation. The
focused runtime allocation tests pass 2/2, and the full `ci-full` gate is being
rerun on this corrected tree.

### chantier 2 hostile review round 21: typed lambda fall-through

The fresh exact-dispatch hostile review returned `NON PROVEN`: the requested
`gpt-5.6-luna` with `max` was not the model actually running, so its verdict is
not closure evidence. Its technical probe was nevertheless reproducible. A
lambda annotated `fn() -> int {}` compiled and executed as `unit`; the same
hole was reachable through `Result` and `Option` combinators. The review also
confirmed the first Miri allocation guard had the same non-deterministic
threshold loop.

A compile-fail guard was added for the typed lambda. Before the sema fix, the
focused command
`cargo test -p aelys --test error_handling_tests typed_lambda_cannot_fall_through -- --nocapture`
exited 101 because `compile_message` panicked with `the source must be rejected`.
The lambda inference path now uses the named-function fall-through check and
infers an empty unannotated lambda as `unit`; the guard passes. Both allocation
guards now call explicit `collect()` rather than waiting for an exact threshold.
The corrected `cargo xtask ci-full` then passed: debug and release workspace
tests, Miri 5/5, AddressSanitizer 5/5, fuzz smoke (9244 runs), and bench
compilation. The exact consumer row also passed 146/0 after the fix.

### chantier 2 hostile review round 22: dynamic `?` boundary RED

The next hostile review was again `NON PROVEN`: the service exposed only
Codex/GPT-5, not the required `gpt-5.6-luna` with `max`, so its verdict cannot
close the phase. Its executed probe found a silent acceptance the local guards
missed: `Result<int,dynamic> -> Result<int,dynamic>` and
`Option<dynamic> -> Option<dynamic>` propagated with `?` and ran, each yielding
the observed integer `0`.

A compile-fail guard was added for both forms. Before the fix, the focused
command
`cargo test -p aelys --test error_handling_tests question_mark_rejects_dynamic_boundaries -- --nocapture`
exited 101 because `compile_message` panicked with `the source must be rejected`.
Try validation now rejects any dynamic nested in either source or target sum
type; the guard passes with the named `cannot propagate` diagnostic.

### chantier 2 revision round 23: private module access was a null leak

The local null audit found that an Aelys source expression could still reach the
VM's legacy null sentinel without spelling `null`: after `needs private_mod`,
`private_mod::secret` referred to a non-public declaration. The existing module
test expected that value to be null, which violates the stage bar.

The test was changed to require a compile-time diagnostic. Before the export
check was added, the exact focused command
`cargo test -p aelys --test module_tests test_private_function_access_is_rejected -- --nocapture`
exited 101 with `private module members must be rejected: null`.

The type checker now carries the imported qualified export set and emits E313:
`module member 'private_mod::secret' is not public; add 'pub' to its declaration`.
The module suite passes 30/30, including aliased, nested, standard, repeated,
and direct imports. This closes one surface null path; internal bytecode and
host ABI null sentinels remain outside the source language and are still tested
as internal behavior.

The fresh exact hostile reviewer Euler opened with Assertion 0 and returned
`NON PROVEN` before repository inspection because the runtime did not expose
exactly `gpt-5.6-luna` with reasoning `max`. No technical verdict was accepted.

The full `cargo xtask ci-full` run immediately before this round was green:
debug and release workspace tests, Miri 5/5, ASAN 5/5, fuzz smoke 8339 runs,
and bench compilation. Because round 23 changed sema, driver, and tests, that
gate must be rerun before any error-handling closure claim.

The first post-fix `cargo xtask ci` exposed one over-rejection in the same
export check: the native fixture exports the nested name `b::c`, so the
intermediate namespace `native_test::b` is not itself an export. The exact
failure was `module member 'native_test::b' is not public` in
`script_imports_native_module`. The checker now accepts an exported namespace
prefix while still rejecting an unknown leaf; the focused native-module test
passes again. This is recorded as a revision to round 23, not as a new surface
feature.

After that revision, `cargo xtask ci` passed formatting, Clippy with warnings
denied, the all-features workspace suite, and diff checks. The consumer was
rerun with `cargo test --workspace --quiet` and again passed 146/0 in groups
25, 94, 4, 8, 7, and 8. A fresh `ci-full` is still required because the last
full gate predates this private-export correction.

The corrected-tree `cargo xtask ci-full` then passed end to end: formatting,
Clippy, debug all-features tests, release all-features tests, Miri 5/5, the
targeted runtime test 5/5, fuzz smoke with 8870 runs, and bench compilation.

### chantier 2 revision round 24: null in pattern diagnostics

The local null audit found one diagnostic hole: expression and type-position
`null` already produced E106 with the Option/Result replacement, but
`match Some(1) { null => 1, _ => 0 }` fell through the generic pattern parser.
The added compile-fail guard was deliberately run before the fix:
`cargo test -p aelys --test error_handling_tests null_pattern_is_rejected_with_the_same_diagnostic -- --nocapture`
exited 101 and printed `error[E0107]: expected a match pattern`.

The pattern parser now maps `TokenKind::Null` to E106. The focused guard passes,
and the complete error rejection suite passes 75/75. No alternate model or
substitute review verdict is used for phase closure.

The post-round gates are green: `cargo xtask ci-full` completed debug and
release workspace tests, Miri 5/5, the nightly target runtime suite 5/5, fuzz
smoke with 9103 runs, and benchmark compilation. The exact
`/home/vbxq/sources/raizen_core` command `cargo test --workspace --quiet` also
passed 146/0.

### chantier 2 revision round 25: alternative-pattern binding emission

The backend audit found that an `Or` pattern compiled its alternative tests a
second time while emitting bindings and emitted an unreachable internal
`MatchFail`. The sema already proves that every alternative binds the same
names and types, so binding from the first alternative is sufficient after the
single shared test sequence. The backend now avoids the duplicate tests and
the dead failure path.

The existing execution guard
`or_pattern_bindings_execute_on_the_selected_alternative` and the complete
error execution suite pass 21/21. After the backend correction, `cargo xtask
ci` passed and the exact consumer suite again passed 146/0. A fresh `ci-full`
is required because the previous full gate predates this backend correction;
the chantier remains open until that gate and the exact hostile review both
complete.

### chantier 2 hostile review round 26: non-proven verifier and native contracts

The corrected-tree `cargo xtask ci-full` completed successfully after round 25:
debug and release workspace tests, Miri 5/5, the nightly target runtime suite
5/5, fuzz smoke with 6283 runs, and benchmark compilation. The exact consumer
row had already passed 146/0 after the backend correction and is being rerun
after this review.

The required exact hostile reviewer opened with Assertion 0 and returned
`NON PROVEN`; no substitute model was used. Its technical audit found a compact
bytecode verifier hole: `MatchFail` checked the message register only for flag
`1`, accepted other flags, and did not constrain the failure family, while the
wide verifier rejected invalid flags. It also found that `sys::set_cwd` and the
`sys::exec*` functions expose scalar or unit signatures even though process and
filesystem failures still become runtime errors. The phase cannot close with
either defect present. The compact verifier guard and typed native failure
guards are now being added before the next review.

### chantier 2 revision round 27: close the verifier and sys failure leaks

The new compact-bytecode guard was run before its verifier fix:
`cargo test -p aelys --test security_audit_tests verifier_rejects_invalid_compact_match_failure_encoding -- --nocapture`
exited 101 because malformed `MatchFail` reached dispatch as
`SumUnwrapFailed { family: "Option", message: None }` instead of being rejected.
The two new native contract guards were also run before their fixes. The
`sys_set_cwd_failure_is_a_result` command exited 101 with
`unknown variant 'Ok' for unit`, and `sys_exec_args_failure_is_a_result`
exited 101 with `unknown variant 'Ok' for i64`.

The compact verifier now rejects unknown match-failure families and flags in
both compact and wide encodings. `sys::cwd`, `set_cwd`, `exec`,
`exec_output`, `exec_args`, `exec_args_output`, and `random_int` now return
typed `Result` sums, including OS and argument failures as `Err(message)`.
The Aelys tests, runtime/JIT seed fixtures, examples, and the consumer's
seeded bridge source were migrated to match or explicitly unwrap those values.
The focused verifier guard passes, `stdlib_sys_tests` passes 28/28,
`runtime_v2_tests` passes 12/12, and the seeded JIT guard passes 1/1. A full
`cargo xtask ci`, exact consumer run, and a fresh hostile review are still
required.

### chantier 2 hostile review round 28: timing flake and foreign ABI gap

The fresh exact hostile review again opened with Assertion 0 and returned
`NON PROVEN`; the requested exact model configuration was not verifiable and
no substitute verdict is accepted. It confirmed the round 27 verifier and sys
fixes, but reproduced an intermittent failure in
`raizen_soak_replays_globals_allocations_and_gc_deterministically`: the test's
`gc_pause_max_ns < 2_000_000` assertion failed under load while a targeted rerun
passed. That wall-clock ceiling does not prove semantic determinism and made
the acceptance gate flaky, so it was removed while allocation and collection
assertions remain.

The reviewer also identified a remaining §1 boundary gap: the foreign native
ABI exposes only scalar, unit, and dynamic signature types, and a nonzero
foreign status still becomes a host `NativeError` instead of an Aelys
`Result` value. The native ABI needs an explicit typed Option/Result contract
before the chantier can be called proven.

### chantier 2 revision round 29: typed foreign Option and Result ABI

The new native wrapper guard was deliberately run before implementation:
`cargo test -p aelys-native --test value_and_wrapper_tests generated_sum_wrappers_encode_option_and_result_statuses -- --nocapture`
exited 101 because the macro reported
`unsupported return type: Result<i64,String>`. The runtime contract guard was
then run with the old raw foreign-success path and exited 101 at
`success.as_ptr().unwrap()`, proving that a scalar foreign return was not being
lifted into an Aelys sum.

The ABI is now version 5 with typed `Option<T>` and `Result<T, String>` return
codes for scalar, string, and unit payloads. The export macro encodes those
types, the VM allocates strings through the native context, and foreign calls
wrap success as `Some`/`Ok` or status values as `None`/`Err` instead of raising
`NativeError`. Dynamic, CLI, and statically registered native functions carry
the result contract into the VM. The native wrapper suite passes 5/5 and the
runtime foreign-value guard passes 1/1. Full gates and consumer acceptance are
required again after this ABI change.

### chantier 2 revision round 30: exercise string payloads at the ABI boundary

The first typed ABI implementation compiled only numeric sum payloads in its
guard. A compile audit found that the generated `String` return path passed
bytes to the string allocator, and Result errors discarded their String
payload before the VM could build `Err`. The generated `Option<String>` and
`Result<String, String>` functions were added to the native fixture; the
focused wrapper suite then exposed the implementation path and was corrected.

The wrapper now allocates returned strings through the VM context and writes a
Result error string to the ABI output when allocation succeeds. The VM imports
that output as the `Err` payload, with a typed fallback message if a foreign
module cannot allocate its diagnostic. The complete native wrapper suite passes
5/5 and the runtime foreign-value guard passes 1/1. The previous full gate
predates this correction and must be repeated.

After the string-path correction, `cargo xtask ci` passed formatting, Clippy,
the all-features workspace suite, and diff checks. The exact consumer command
again passed 25, 94, 4, 8, 7, 0, 0, 8, 0, 0, 0, and 0 tests: 146 passed and
0 failed. A fresh full gate and exact hostile review are the remaining closure
checks for chantier 2.

### chantier 2 revision round 31: execute the hostile review's missing ABI cases

The exact-config hostile review returned `NON PROVEN`. It confirmed the full
gate and consumer, but found no executable wide `MatchFail` verifier test and
no direct execution of String or unit Option/Result ABI paths, including an
Err String payload.

The new wide verifier test was run against the existing verifier guard, then
the two wide checks were temporarily removed. The required RED run exited 101
with `expected InvalidBytecode, got SumUnwrapFailed { family: "sum",
message: None }`. Restoring the checks made both compact and wide rejection
tests pass.

The native fixture now exercises Option<String>, Result<String, String>,
Option<()>, and Result<(), String>. Before retaining the implementation, the
Result error allocation was temporarily removed; the wrapper test exited 101
with `left: 1, right: 4`, proving the Err payload was not preserved. The unit
return cases were then temporarily removed from the macro; compilation exited
101 with `unsupported return type: Option<()>` and missing generated wrapper
symbols. The implementation was restored.

The runtime guard now calls actual foreign callbacks for String and unit
successes, Option None, Result Ok, and Result Err payloads. With Err payload
import temporarily removed, it exited 101 because the fallback diagnostic was
returned instead of `"error"`. Restored code passes the guard. The native
wrapper suite is now 6/6, the runtime suite has the new typed-payload test
passing, and the two compact/wide verifier tests pass. The complete gates and
consumer acceptance must be repeated before closure.

### chantier 2 revision round 32: local gates green, exact reviewer unavailable

The requested fresh reviewer could not be dispatched in this execution: the
sub-agent capability was not exposed. No substitute model was used, so Assertion 0
requires `NON PROVEN`.

The local checks completed independently: compact and wide MatchFail rejection 2/2,
native wrappers 6/6, foreign String and unit payloads 1/1, compile-time rejection
75/75, runtime error handling 21/21, security audit 39 passed with 1 ignored, and
the consumer workspace 146 passed with 0 failures.

The repeated `cargo xtask ci-full` passed formatting, Clippy, debug and release
workspace suites, Miri 7/7, target tests 7/7, fuzz smoke with 6177 runs, and bench
compilation. The timing assertion remains absent; no semantic result depends on it.

The chantier remains open until a fresh reviewer actually runs as
`gpt-5.6-luna` with reasoning `max` and returns `PROVEN` with its own evidence.

### chantier 2 revision round 33: exact-config retry with priority service

A fresh reviewer was dispatched with `gpt-5.6-luna`, reasoning `max`, and the
priority service tier. It returned `NON PROVEN` before technical audit because
the exact model and effort were still unavailable in the execution. No
substitute model was used and no files were modified. The chantier remains
open; local implementation work is not a substitute for the required exact
hostile review.

### chantier 2 revision round 34: reassignment must-use dataflow

A local hostile audit found that a Result binding could be read and then
reassigned through an explicitly discarded assignment expression without the
new Result value being tracked. The new compile-fail test initially ran against
that gap and exited 101 because `compile_message` reported `the source must be
rejected`.

The must-use analyzer now treats assignment as a new value generation: it
resets the binding's used state and records the assignment span, while the
right-hand side is still visited first. The guard now passes with the named
`unused Result value` diagnostic. The full error rejection suite is 76/76;
workspace and consumer gates are required again after this correction.

### chantier 2 revision round 35: assignment-result consumption

The generation reset initially over-rejected valid uses. A compile-success
guard for `(value = Ok(2)).unwrap()` exited 101 with a named `unused Result
value` diagnostic at the assignment. A second compile-success guard for
`return value = Ok(2)` reproduced the same rejection.

The analyzer now recognizes a direct or parenthesized assignment result when
it is the receiver of a member call and when it is returned, marking that new
generation as consumed. Both guards pass, while the round 34 discard guard
continues to reject. The error suite is now 78/78; acceptance gates are still
required.

The consumption follow-up also routes call arguments, operators, indexing,
literal fields, ranges, guards, and `?` operands through the same assignment
generation check. Parenthesized assignment results remain consumable without
weakening the explicit discard diagnostic. The 78-test rejection suite and all
three assignment guards remain green.

After this correction, `cargo xtask ci` passed with Clippy and the full
workspace suite, the consumer again passed 146/0, and `cargo xtask ci-full`
passed through release, Miri 7/7, target tests 7/7, fuzz smoke `6736` runs,
and bench compilation. The exact review requirement remains the only open
closure condition.

### chantier 2 revision round 36: exact review blockers and RED guards

The exact `gpt-5.6-luna` reviewer ran the required suites and returned
`NON PROVEN`. Its commands were independently green: error rejection 78/78,
error execution 21/21, native wrappers 6/6, typed String/unit ABI 1/1,
security 39 passed with 1 ignored, native integration 31 + 1 + 2 + 2 + 1,
`cargo xtask ci-full` with fuzz smoke 6328 runs, and the exact consumer 146/0.

It found two soundness blockers. Foreign return handling retains only the
Option or Result family and does not validate the declared scalar payload, so
a declared `Result<String, String>` can carry an integer and a declared
`Option<String>` can carry the null sentinel. Sized object arrays also create
null-filled slots: `Array<String>(1)` type checks and `ArrayNewP` returns the
slot through a typed string expression.

Two guards were added before the fixes. The compile-fail guard for
`Array<String>(1)` exited 101 at `compile_message` with `the source must be
rejected`, proving the compiler accepted the null-producing form. The runtime
ABI guard exited 101 at `unwrap_err` with `called Result::unwrap_err() on an Ok
value: <ptr:129>`, proving a wrong integer payload was wrapped as a successful
Result. The null-payload half of that guard was not reached because the first
assertion failed. These are the required RED observations; implementation
changes follow.

The native ABI now carries the exact scalar payload contract for every typed
foreign return. Scalar, Option, and both Result branches validate integers,
floats, booleans, units, and heap strings before exposing a value or wrapping
it in a sum. A foreign Result error allocates its declared String payload
through the VM API, and null remains rejected.

Sized arrays now accept only primitive element types with a real zero value.
Object, Dynamic, Option, Result, and other non-defaultable element types are
rejected with E0314 before `ArrayNewP` can create null-filled slots. The two
RED guards now pass: the native contract test and the compile-time
`Array<String>(1)` rejection test both exit successfully.

### chantier 2 revision round 37: accepted slicing reaches a backend panic

The exact hostile audit completed the full gates and consumer row but returned
`NON PROVEN` because `[1, 2, 3][0..2]` is accepted by the parser and sema, then
reaches `todo!("slice")` in the backend. A live execution guard was added before
the fix. `cargo test -p aelys --test error_handling_runtime_tests
slicing_executes_without_reaching_the_backend_panic -- --nocapture` exited 101
with `not yet implemented: slice`, one failed and zero passed. The slice path
must be implemented before another closure review.

### chantier 2 revision round 38: slice surface expansion red observation

Two additional live guards were written for inclusive and open-ended Vec
slices and for invalid bounds. Before the implementation was restored, the
complete runtime suite was run once with both slice compiler methods disabled:
24 tests ran, 21 passed, and 3 failed. The three failures were
`invalid_slice_bounds_raise_a_runtime_error`,
`slicing_executes_without_reaching_the_backend_panic`, and
`vector_slicing_supports_inclusive_and_open_bounds`; each exited 101 at
`backend/src/compiler/expr/typed/array.rs:245:9` with `not yet implemented:
slice`. This records that the new guards fail against the missing behavior.

The slice path now materializes a GC-managed range object, emits dedicated
`RangeNew`, `RangeNewInclusive`, `ArraySlice`, and `VecSlice` operations, and
normalizes omitted, inclusive, reversed, negative, and out-of-range bounds
before copying a collection. Array and Vec payloads retain their storage
specialization, and object payloads remain reachable through the source
collection register during allocation. The live runtime suite is green at
24/24, including Array slicing, Vec slicing, inclusive and open bounds, and a
runtime error for invalid bounds. The compile rejection suite remains 79/79.
