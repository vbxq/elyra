use aelys_common::error::CompileErrorKind;
use aelys_sema::constraint::{Constraint, ConstraintReason, TypeErrorKind};
use aelys_sema::types::{InferType, TypeVarId};
use aelys_syntax::Span;

#[test]
fn test_constraint_creation() {
    let c = Constraint::equal(
        InferType::I64,
        InferType::F64,
        Span::dummy(),
        ConstraintReason::BinaryOp {
            op: "+".to_string(),
        },
    );
    assert!(matches!(c, Constraint::Equal { .. }));
}

#[test]
fn test_one_of_constraint() {
    let c = Constraint::one_of(
        InferType::Var(TypeVarId(0)),
        vec![InferType::I64, InferType::F64, InferType::String],
        Span::dummy(),
        ConstraintReason::BinaryOp {
            op: "+".to_string(),
        },
    );
    assert!(matches!(c, Constraint::OneOf { .. }));
}

#[test]
fn test_type_error_display() {
    let err = aelys_sema::constraint::TypeError::mismatch(
        InferType::I64,
        InferType::F64,
        Span::dummy(),
        ConstraintReason::BinaryOp {
            op: "+".to_string(),
        },
    );
    let msg = format!("{}", err);
    assert!(msg.contains("type mismatch"));
    assert!(msg.contains("i64"));
    assert!(msg.contains("f64"));
}

#[test]
fn the_mangled_symbol_collision_keeps_its_own_code() {
    let kind = TypeErrorKind::MangledSymbolCollision {
        name: "probe$i64".to_string(),
    };
    assert_eq!(kind.diagnostic_code(), 355);
}

#[test]
fn the_enum_layout_diagnostic_keeps_code_346() {
    let kind = TypeErrorKind::EnumLayoutTooLarge {
        enum_name: "Huge".to_string(),
        item: "variant 'Payload'".to_string(),
        count: 65536,
        limit: 65535,
    };
    assert_eq!(kind.diagnostic_code(), 346);
}

#[test]
fn the_module_path_separator_renders_a_single_code() {
    let kind = TypeErrorKind::ModulePathSeparator {
        module: "sys".to_string(),
        member: "arch".to_string(),
    };
    assert_eq!(kind.diagnostic_code(), 411);
    assert_eq!(
        kind.diagnostic_code(),
        CompileErrorKind::ModulePathSeparator {
            module: "sys".to_string(),
            member: "arch".to_string(),
        }
        .code()
    );
}

#[test]
fn the_unreachable_pattern_diagnostic_has_its_own_code() {
    let kind = TypeErrorKind::UnreachablePattern {
        pattern: "Shape::A".to_string(),
    };
    assert_eq!(kind.diagnostic_code(), 356);
}

#[test]
fn the_named_inference_diagnostics_do_not_collapse_into_the_generic_code() {
    let named: Vec<(TypeErrorKind, u16)> = vec![
        (
            TypeErrorKind::GenericArityMismatch {
                name: "Pair".to_string(),
                expected: 2,
                found: 1,
            },
            357,
        ),
        (
            TypeErrorKind::PatternBindingMismatch {
                expected: vec!["a".to_string()],
                found: vec!["b".to_string()],
            },
            358,
        ),
        (
            TypeErrorKind::ArityMismatch {
                expected: 1,
                found: 2,
            },
            359,
        ),
        (TypeErrorKind::NotCallable { ty: InferType::I64 }, 360),
        (
            TypeErrorKind::InfiniteType {
                var: TypeVarId(0),
                ty: InferType::I64,
            },
            361,
        ),
        (
            TypeErrorKind::UndefinedFunction {
                name: "probe".to_string(),
            },
            362,
        ),
        (
            TypeErrorKind::UnknownField {
                structure: "Point".to_string(),
                field: "z".to_string(),
            },
            363,
        ),
        (
            TypeErrorKind::MissingField {
                structure: "Point".to_string(),
                field: "y".to_string(),
            },
            364,
        ),
        (
            TypeErrorKind::NotIterable {
                receiver: InferType::I64,
            },
            365,
        ),
        (
            TypeErrorKind::InvalidIndex {
                receiver: InferType::I64,
            },
            366,
        ),
        (
            TypeErrorKind::UntypedNativeTypeMismatch {
                name: "raw".to_string(),
                expected: InferType::I64,
            },
            367,
        ),
        (TypeErrorKind::RecursionLimit, 368),
        (
            TypeErrorKind::UnknownVariant {
                variant: "Shape::Z".to_string(),
                expected: "Shape".to_string(),
            },
            109,
        ),
        (TypeErrorKind::MatchArmValueRequired, 110),
        (
            TypeErrorKind::MissingReturnValue {
                expected: InferType::I64,
            },
            215,
        ),
    ];
    for (kind, expected) in named {
        assert_eq!(
            kind.diagnostic_code(),
            expected,
            "unexpected code for {kind:?}"
        );
    }
}

#[test]
fn the_generic_code_stays_reserved_for_plain_inference_failures() {
    assert_eq!(
        TypeErrorKind::Mismatch {
            expected: InferType::I64,
            found: InferType::F64,
        }
        .diagnostic_code(),
        301
    );
    assert_eq!(
        TypeErrorKind::UndefinedVariable {
            name: "type".to_string(),
        }
        .diagnostic_code(),
        301
    );
}
