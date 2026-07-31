//! Integration tests: doc-comment extraction and lib+bin FunctionId keys.

use horizon_engine::{DocCommentKind, File, Folder, build_function_map, map_to_string};
use std::collections::HashSet;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is nested under crates/<name>")
        .to_path_buf()
}

fn fixture(name: &str) -> PathBuf {
    workspace_root().join("tests/fixtures")
        .join(name)
}

fn all_files(krate: &horizon_engine::Crate) -> Vec<&File> {
    let mut out = Vec::new();
    fn walk<'a>(files: &'a [File], folders: &'a [Folder], out: &mut Vec<&'a File>) {
        out.extend(files.iter());
        for folder in folders {
            walk(&folder.files, &folder.folders, out);
        }
    }
    walk(&krate.files, &krate.folders, &mut out);
    out
}

#[test]
fn doc_comments_fixture_exact_text() {
    let map = build_function_map(fixture("doc-comments")).expect("build map");
    assert_eq!(map.crates.len(), 1);
    let krate = &map.crates[0];
    let files = all_files(krate);
    assert_eq!(files.len(), 1);
    let file = files[0];

    assert_eq!(file.doc_comments.len(), 1);
    assert_eq!(file.doc_comments[0].kind, DocCommentKind::Inner);
    assert_eq!(
        file.doc_comments[0].text,
        "File-level module docs for the doc-comments fixture.\nSecond inner line."
    );

    let by_name = |name: &str| {
        file.functions
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("missing {name}"))
    };

    assert_eq!(
        by_name("alpha").doc_comments[0].text,
        "Single-line outer docs on alpha."
    );
    assert_eq!(
        by_name("beta").doc_comments[0].text,
        "First outer line on beta.\nSecond outer line on beta."
    );
    assert_eq!(
        by_name("gamma").doc_comments[0].text,
        "Docs separated from the item by an attribute."
    );
    assert_eq!(
        by_name("delta").doc_comments[0].text,
        "Block outer docs on delta."
    );
    assert!(
        by_name("epsilon").doc_comments.is_empty(),
        "ordinary // must not be collected"
    );
    assert_eq!(
        by_name("zeta").doc_comments[0].text,
        "Attribute-form docs on zeta."
    );

    // JSON excerpt: a documented function.
    let alpha = by_name("alpha");
    let excerpt = serde_json::json!({
        "id": alpha.id.as_str(),
        "name": alpha.name,
        "doc_comments": alpha.doc_comments,
    });
    let excerpt_s = serde_json::to_string_pretty(&excerpt).unwrap();
    assert!(excerpt_s.contains("Single-line outer docs on alpha."));
    assert!(excerpt_s.contains("\"kind\": \"outer\""));

    let json = map_to_string(&map).expect("serialize");
    assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
}

#[test]
fn lib_and_bin_function_ids_do_not_collide_or_fabricate_crate_name() {
    let map = build_function_map(fixture("lib-and-bin")).expect("build map");
    assert_eq!(map.crates.len(), 2);

    let lib = map
        .crates
        .iter()
        .find(|c| c.is_library)
        .expect("library crate");
    let bin = map
        .crates
        .iter()
        .find(|c| !c.is_library)
        .expect("binary crate");

    assert_eq!(lib.rustc_name, "lib_and_bin");
    assert_eq!(bin.rustc_name, "lib_and_bin");
    assert_eq!(lib.function_id_prefix(), "lib_and_bin");
    assert_eq!(bin.function_id_prefix(), "lib_and_bin[bin]");

    let lib_files = all_files(lib);
    let bin_files = all_files(bin);
    let helper = lib_files
        .iter()
        .flat_map(|f| f.functions.iter())
        .find(|f| f.name == "helper")
        .expect("helper");
    let main = bin_files
        .iter()
        .flat_map(|f| f.functions.iter())
        .find(|f| f.name == "main")
        .expect("main");

    assert_eq!(helper.id.as_str(), "lib_and_bin::helper");
    assert_eq!(main.id.as_str(), "lib_and_bin[bin]::main");
    assert_ne!(helper.id, main.id);

    let ids: HashSet<&str> = map
        .crates
        .iter()
        .flat_map(|c| all_files(c))
        .flat_map(|f| f.functions.iter())
        .map(|f| f.id.as_str())
        .collect();
    assert_eq!(ids.len(), 2, "ids must be unique across lib+bin");

    for id in &ids {
        let crate_key = id.split("::").next().unwrap_or(id);
        assert!(
            crate_key == "lib_and_bin" || crate_key == "lib_and_bin[bin]",
            "crate key must be real name or name[bin], not a fabricated *_bin name; got {id}"
        );
        assert_ne!(
            crate_key, "lib_and_bin_bin",
            "must not invent fabricated crate name lib_and_bin_bin"
        );
    }
    assert!(ids.iter().any(|id| id.starts_with("lib_and_bin[bin]::")));
}
