use aelys_bytecode::asm::binary::{deserialize_with_manifest, serialize_with_manifest};
use aelys_runtime::Function;

#[test]
fn avbc_round_trip_manifest() {
    let func = Function::new(None, 0);
    let manifest = b"[build]\nbundle_native_modules = true\n".to_vec();

    let bytes = serialize_with_manifest(&func, Some(&manifest), None).unwrap();
    let (_func, decoded_manifest, _bundles) = deserialize_with_manifest(&bytes).expect("read");

    assert_eq!(decoded_manifest.unwrap(), manifest);
}

#[test]
fn avbc_round_trip_required_imports() {
    use aelys_bytecode::asm::{
        RequiredImport, RequiredImportKind, deserialize_with_sections, serialize_with_sections,
    };

    let func = Function::new(None, 0);
    let requires = vec![
        RequiredImport {
            path: vec!["holder".to_string()],
            kind: RequiredImportKind::Module { alias: None },
        },
        RequiredImport {
            path: vec!["lib".to_string(), "helper".to_string()],
            kind: RequiredImportKind::Module {
                alias: Some("h".to_string()),
            },
        },
        RequiredImport {
            path: vec!["std".to_string(), "math".to_string()],
            kind: RequiredImportKind::Symbols(vec!["sqrt".to_string(), "pow".to_string()]),
        },
        RequiredImport {
            path: vec!["wide".to_string()],
            kind: RequiredImportKind::Wildcard,
        },
    ];

    let bytes = serialize_with_sections(&func, None, None, &requires).unwrap();
    let sections = deserialize_with_sections(&bytes).expect("read");

    assert_eq!(sections.requires, requires);
}

#[test]
fn an_artifact_without_the_section_reads_back_with_no_required_import() {
    use aelys_bytecode::asm::deserialize_with_sections;

    let func = Function::new(None, 0);
    let bytes = serialize_with_manifest(&func, None, None).unwrap();

    assert!(
        deserialize_with_sections(&bytes)
            .expect("read")
            .requires
            .is_empty()
    );
}

#[test]
fn a_truncated_required_import_section_is_refused() {
    use aelys_bytecode::asm::{
        RequiredImport, RequiredImportKind, deserialize_with_sections, serialize_with_sections,
    };

    let func = Function::new(None, 0);
    let requires = vec![RequiredImport {
        path: vec!["holder".to_string()],
        kind: RequiredImportKind::Module { alias: None },
    }];
    let bytes = serialize_with_sections(&func, None, None, &requires).unwrap();

    for cut in 1..12 {
        let truncated = &bytes[..bytes.len() - cut];
        assert!(
            deserialize_with_sections(truncated).is_err(),
            "a section cut {cut} bytes short must be refused, not guessed"
        );
    }
}
