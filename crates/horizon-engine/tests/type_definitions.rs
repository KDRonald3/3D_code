//! W8: type definitions are first-class map nodes with honest type_refs.

use horizon_engine::{TypeKind, TypeTarget, build_function_map};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn fixture(name: &str) -> PathBuf {
    workspace_root().join("tests/fixtures").join(name)
}

#[test]
fn emits_structs_enums_aliases_and_field_type_refs() {
    let map = build_function_map(fixture("type-definitions")).expect("build map");
    assert_eq!(map.crates.len(), 1);
    let file = &map.crates[0].files[0];

    let names: Vec<&str> = file.types.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"Point"), "{names:?}");
    assert!(names.contains(&"Named"), "{names:?}");
    assert!(names.contains(&"Label"), "{names:?}");
    assert!(names.contains(&"Tag"), "{names:?}");
    assert!(names.contains(&"Alias"), "{names:?}");

    let named = file
        .types
        .iter()
        .find(|t| t.name == "Named")
        .expect("Named");
    assert_eq!(named.kind, TypeKind::Struct);
    assert_eq!(named.module_path, "crate::Named");
    assert!(named.id.as_str().ends_with("::Named"));

    let ref_names: Vec<&str> = named
        .type_refs
        .iter()
        .map(|r| r.type_path.as_str())
        .collect();
    assert!(
        ref_names.iter().any(|p| *p == "Label" || p.ends_with("::Label")),
        "Named should name Label: {ref_names:?}"
    );
    assert!(
        ref_names.iter().any(|p| *p == "Point" || p.ends_with("::Point")),
        "Named should name Point: {ref_names:?}"
    );

    for r in &named.type_refs {
        assert!(
            matches!(r.target, TypeTarget::Resolved(_)),
            "field type `{}` should resolve: {:?}",
            r.type_path,
            r.target
        );
    }

    let alias = file
        .types
        .iter()
        .find(|t| t.name == "Alias")
        .expect("Alias");
    assert_eq!(alias.kind, TypeKind::TypeAlias);
    assert_eq!(alias.type_refs.len(), 1);
    assert!(matches!(
        &alias.type_refs[0].target,
        TypeTarget::Resolved(id) if id.as_str().ends_with("::Named")
    ));

    let label = file
        .types
        .iter()
        .find(|t| t.name == "Label")
        .expect("Label");
    assert_eq!(label.kind, TypeKind::Enum);
    assert!(label.variants.iter().any(|v| v == "Short"));
    assert!(
        label
            .type_refs
            .iter()
            .any(|r| matches!(r.target, TypeTarget::Resolved(_))),
        "Label::Short(Tag) should resolve Tag: {:?}",
        label.type_refs
    );

    // Free functions still present; constructors remain dropped, not unresolved.
    assert!(file.functions.iter().any(|f| f.name == "make"));
    assert_eq!(map.summary.unresolved, 0);
}
