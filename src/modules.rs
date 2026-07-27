//! Stage 3: walk the `mod` tree to find files that belong to a crate.
//!
//! From each compilation root, follow `mod` declarations (honouring `#[path]`)
//! to discover which files are in the crate and what their module paths are.
//! Files nobody declares are ignored — directory globbing is wrong here.
//!
//! # Phase 2
//!
//! Declaration-driven transitive walk from the primary compilation root. Inline
//! `mod name { ... }` blocks contribute a module-path level but no `File` node.
//! Missing module files are reported and skipped (never panic). Cycles and
//! repeated declarations terminate that branch.
//!
//! # Macro-hidden `mod` recovery
//!
//! Allowlisted item-pasting macros (`cfg_if!`, Tokio-style `cfg_*!`, … — see
//! [`crate::extract::is_item_macro_allowlisted`]) hide `mod` declarations inside
//! token trees. Recovery re-parses the token-tree interior (same discipline as
//! macro-hidden *call* recovery in [`crate::extract`]) and walks the resulting
//! modules. Nested allowlisted macros are opened up to
//! [`MAX_MACRO_MOD_DEPTH`]. `cfg_if!` branches are **unioned** (no build-cfg
//! choice); duplicate module paths resolving to the same file dedupe via the
//! ordinary seen-path tables.
//!
//! # Phase 4
//!
//! Records each module's visibility (`pub mod` vs `mod`, …). Cross-crate
//! resolution consults the full module-chain visibility, not only the item's
//! own marker — a `pub fn` inside a private module is unreachable from outside.

use crate::discover::normalize_path;
use crate::extract::{
    ItemVisibility, is_item_macro_allowlisted, is_inside_macro_definition, macro_call_name,
};
use crate::map::{Crate, File};
use crate::parse::parse_source;
use anyhow::{Result, bail};
use ra_ap_syntax::ast::{
    self, AstNode, HasAttrs, HasModuleItem, HasName, HasVisibility, LiteralKind, VisibilityKind,
};
use ra_ap_syntax::{SyntaxKind, SyntaxNode};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Bound nested macro-mod recovery (`cfg_net! { cfg_net_unix! { mod unix; } }`).
/// Matches the call-recovery depth in [`crate::extract`].
const MAX_MACRO_MOD_DEPTH: usize = 8;

/// Synthetic module wrapping a re-parsed macro token tree.
const MACRO_MOD_PROBE: &str = "__horizon_mod_probe";

/// A source file known to belong to a crate, with its module path.
#[derive(Debug, Clone)]
pub struct ModuleFile {
    pub file: File,
}

/// Result of walking the crate's module tree.
#[derive(Debug, Clone, Default)]
pub struct ModuleWalk {
    /// Files reached by `mod` declarations (and the crate root), each once.
    pub files: Vec<ModuleFile>,
    /// Every module path encountered, including inline modules with no file.
    pub module_paths: HashSet<String>,
    /// Visibility of each module path (`pub mod` / `mod` / …). The crate root
    /// is recorded as [`ItemVisibility::Public`] (the crate itself is the
    /// entry point for foreign crates).
    pub module_visibility: HashMap<String, ItemVisibility>,
}

struct Walker {
    edition: String,
    walk: ModuleWalk,
    seen_module_paths: HashSet<String>,
    seen_file_modules: HashSet<(PathBuf, String)>,
}

/// Walk `mod` declarations from the crate's primary root and return the files
/// that participate in compilation, plus the set of module paths.
pub fn walk_modules(krate: &Crate) -> Result<ModuleWalk> {
    let Some(root) = primary_root(krate) else {
        bail!(
            "crate `{}` has no library or binary root to analyse",
            krate.name
        );
    };

    let root = normalize_path(&root);
    let mut walker = Walker {
        edition: krate.edition.clone(),
        walk: ModuleWalk::default(),
        seen_module_paths: HashSet::new(),
        seen_file_modules: HashSet::new(),
    };

    walker.walk.module_paths.insert("crate".into());
    walker
        .walk
        .module_visibility
        .insert("crate".into(), ItemVisibility::Public);
    walker.seen_module_paths.insert("crate".to_string());
    walker.push_file(root.clone(), "crate".into());
    walker
        .seen_file_modules
        .insert((root.clone(), "crate".to_string()));

    let child_dir = module_child_dir(&root);
    let path_attr_base = root
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    walker.walk_file_modules(&root, "crate", &child_dir, &path_attr_base)?;
    Ok(walker.walk)
}

/// Prefer the first library root; fall back to the first binary root.
fn primary_root(krate: &Crate) -> Option<PathBuf> {
    krate.roots.first().cloned()
}

impl Walker {
    fn push_file(&mut self, path: PathBuf, module_path: String) {
        self.walk.files.push(ModuleFile {
            file: File {
                path,
                module_path,
                functions: Vec::new(),
                call_sites: Vec::new(),
                doc_comments: Vec::new(),
            },
        });
    }

    fn walk_file_modules(
        &mut self,
        file_path: &Path,
        module_path: &str,
        child_dir: &Path,
        path_attr_base: &Path,
    ) -> Result<()> {
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(err) => {
                eprintln!(
                    "horizon: could not read {}: {err} (module walk skips nested mods)",
                    file_path.display()
                );
                return Ok(());
            }
        };

        let tree = parse_source(&source, &self.edition)?;
        self.walk_items(tree.items(), module_path, child_dir, path_attr_base, 0)
    }

    fn walk_items(
        &mut self,
        items: impl Iterator<Item = ast::Item>,
        module_path: &str,
        child_dir: &Path,
        path_attr_base: &Path,
        macro_depth: usize,
    ) -> Result<()> {
        for item in items {
            match item {
                ast::Item::Module(module) => {
                    self.process_module(
                        &module,
                        module_path,
                        child_dir,
                        path_attr_base,
                        macro_depth,
                    )?;
                }
                ast::Item::MacroCall(mac) => {
                    self.recover_mods_from_macro(
                        &mac,
                        module_path,
                        child_dir,
                        path_attr_base,
                        macro_depth,
                    )?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn process_module(
        &mut self,
        module: &ast::Module,
        module_path: &str,
        child_dir: &Path,
        path_attr_base: &Path,
        macro_depth: usize,
    ) -> Result<()> {
        let Some(name) = module.name() else {
            return Ok(());
        };
        let name = name.text().to_string();
        if name == MACRO_MOD_PROBE {
            return Ok(());
        }
        let child_module_path = extend_module_path(module_path, &name);
        let mod_vis = module_visibility_of(module);

        if !self.seen_module_paths.insert(child_module_path.clone()) {
            // Repeated declaration or cycle — terminate this branch.
            return Ok(());
        }
        self.walk.module_paths.insert(child_module_path.clone());
        self.walk
            .module_visibility
            .insert(child_module_path.clone(), mod_vis);

        let path_attr = path_attribute(module);
        let child_child_dir = child_dir.join(&name);
        let child_path_attr_base = path_attr_base.join(&name);

        if let Some(item_list) = module.item_list() {
            // Inline module: path level only, same declaring file.
            self.walk_items(
                item_list.items(),
                &child_module_path,
                &child_child_dir,
                &child_path_attr_base,
                macro_depth,
            )?;
            return Ok(());
        }

        // `mod name;` — resolve on disk.
        let resolved = resolve_module_file(child_dir, path_attr_base, &name, path_attr.as_deref());
        if !resolved.is_file() {
            eprintln!(
                "horizon: module `{child_module_path}` declares missing file {} (skipped)",
                resolved.display()
            );
            return Ok(());
        }

        let resolved = normalize_path(&resolved);
        if !self
            .seen_file_modules
            .insert((resolved.clone(), child_module_path.clone()))
        {
            return Ok(());
        }

        self.push_file(resolved.clone(), child_module_path.clone());

        let next_child_dir = module_child_dir(&resolved);
        let next_path_base = resolved
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        self.walk_file_modules(
            &resolved,
            &child_module_path,
            &next_child_dir,
            &next_path_base,
        )?;
        Ok(())
    }

    fn recover_mods_from_macro(
        &mut self,
        mac: &ast::MacroCall,
        module_path: &str,
        child_dir: &Path,
        path_attr_base: &Path,
        macro_depth: usize,
    ) -> Result<()> {
        if macro_depth >= MAX_MACRO_MOD_DEPTH {
            return Ok(());
        }
        if is_inside_macro_definition(mac.syntax()) {
            return Ok(());
        }
        let Some(name) = macro_call_name(mac) else {
            return Ok(());
        };
        if !is_item_macro_allowlisted(&name) {
            return Ok(());
        }
        let Some(tt) = mac.token_tree() else {
            return Ok(());
        };
        let raw = tt.syntax().text().to_string();
        if raw.len() < 2 {
            return Ok(());
        }
        let content = &raw[1..raw.len() - 1];
        self.recover_from_macro_content(content, module_path, child_dir, path_attr_base, macro_depth)
    }

    fn recover_from_macro_content(
        &mut self,
        content: &str,
        module_path: &str,
        child_dir: &Path,
        path_attr_base: &Path,
        macro_depth: usize,
    ) -> Result<()> {
        if content.trim().is_empty() {
            return Ok(());
        }

        let wrapped = format!("mod {MACRO_MOD_PROBE} {{ {content} }}");
        let tree = match parse_source(&wrapped, &self.edition) {
            Ok(t) => t,
            Err(_) => return Ok(()),
        };

        let Some(probe) = tree.items().find_map(|item| match item {
            ast::Item::Module(m)
                if m.name()
                    .is_some_and(|n| n.text() == MACRO_MOD_PROBE) =>
            {
                Some(m)
            }
            _ => None,
        }) else {
            return Ok(());
        };

        let next_depth = macro_depth + 1;

        // Ordinary item-pasting macros (`cfg_fs! { pub mod fs; }`): the probe
        // item list holds real items, including nested MacroCall.
        if let Some(item_list) = probe.item_list() {
            let items: Vec<_> = item_list.items().collect();
            if !items.is_empty() {
                return self.walk_items(
                    items.into_iter(),
                    module_path,
                    child_dir,
                    path_attr_base,
                    next_depth,
                );
            }
        }

        // `cfg_if!` branches parse as ERROR nodes; module decls still appear as
        // Module AST nodes under those errors. Take the union of every branch.
        let probe_syntax = probe.syntax().clone();
        for node in probe_syntax.descendants() {
            let Some(module) = ast::Module::cast(node.clone()) else {
                continue;
            };
            if module.syntax() == &probe_syntax {
                continue;
            }
            if inside_non_probe_module(module.syntax(), &probe_syntax) {
                continue;
            }
            self.process_module(&module, module_path, child_dir, path_attr_base, next_depth)?;
        }
        for node in probe_syntax.descendants() {
            let Some(mac) = ast::MacroCall::cast(node) else {
                continue;
            };
            if inside_non_probe_module(mac.syntax(), &probe_syntax) {
                continue;
            }
            self.recover_mods_from_macro(&mac, module_path, child_dir, path_attr_base, next_depth)?;
        }

        Ok(())
    }
}

/// True when `node` sits inside a `mod` other than the synthetic probe wrapper.
fn inside_non_probe_module(node: &SyntaxNode, probe: &SyntaxNode) -> bool {
    for ancestor in node.ancestors().skip(1) {
        if &ancestor == probe {
            return false;
        }
        if ancestor.kind() == SyntaxKind::MODULE {
            return true;
        }
    }
    false
}

fn module_visibility_of(module: &ast::Module) -> ItemVisibility {
    match module.visibility() {
        None => ItemVisibility::Private,
        Some(v) => match v.kind() {
            VisibilityKind::Pub => ItemVisibility::Public,
            VisibilityKind::PubCrate => ItemVisibility::Crate,
            VisibilityKind::PubSuper => ItemVisibility::Super,
            VisibilityKind::PubSelf => ItemVisibility::SelfMod,
            VisibilityKind::In(path) => {
                let segs: Vec<String> = path
                    .segments()
                    .filter_map(|s| s.name_ref().map(|n| n.text().to_string()))
                    .collect();
                ItemVisibility::InPath(segs.join("::"))
            }
        },
    }
}

fn path_attribute(module: &ast::Module) -> Option<String> {
    for attr in module.attrs() {
        if attr.simple_name().as_deref() != Some("path") {
            continue;
        }
        let Some(ast::Meta::KeyValueMeta(kv)) = attr.meta() else {
            continue;
        };
        let Some(expr) = kv.expr() else {
            continue;
        };
        let Some(lit) = ast::Literal::cast(expr.syntax().clone()) else {
            continue;
        };
        if let LiteralKind::String(s) = lit.kind() {
            if let Ok(value) = s.value() {
                return Some(value.into_owned());
            }
        }
        // Fallback: strip quotes from raw token text.
        let raw = lit.syntax().text().to_string();
        let trimmed = raw.trim();
        if let Some(inner) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            return Some(inner.to_string());
        }
    }
    None
}

fn resolve_module_file(
    child_dir: &Path,
    path_attr_base: &Path,
    name: &str,
    path_attr: Option<&str>,
) -> PathBuf {
    if let Some(p) = path_attr {
        return normalize_path(&path_attr_base.join(p));
    }

    let rs = child_dir.join(format!("{name}.rs"));
    if rs.is_file() {
        return normalize_path(&rs);
    }
    let mod_rs = child_dir.join(name).join("mod.rs");
    if mod_rs.is_file() {
        return normalize_path(&mod_rs);
    }
    // Prefer the conventional `.rs` path when recording a missing file.
    normalize_path(&rs)
}

/// Directory in which this module's children (`mod child;`) are sought.
fn module_child_dir(file: &Path) -> PathBuf {
    let parent = file.parent().unwrap_or_else(|| Path::new("."));
    let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if matches!(file_name, "mod.rs" | "lib.rs" | "main.rs") {
        parent.to_path_buf()
    } else if let Some(stem) = file.file_stem() {
        parent.join(stem)
    } else {
        parent.to_path_buf()
    }
}

fn extend_module_path(parent: &str, name: &str) -> String {
    if parent == "crate" {
        format!("crate::{name}")
    } else {
        format!("{parent}::{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn module_child_dir_for_file_module() {
        let p = PathBuf::from("src/app.rs");
        assert_eq!(module_child_dir(&p), PathBuf::from("src/app"));
    }

    #[test]
    fn module_child_dir_for_lib() {
        let p = PathBuf::from("src/lib.rs");
        assert_eq!(module_child_dir(&p), PathBuf::from("src"));
    }

    #[test]
    fn missing_module_file_does_not_panic() {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("horizon-missing-mod-{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let lib = dir.join("lib.rs");
        fs::write(&lib, "mod absent;\npub fn ok() {}\n").unwrap();

        let krate = Crate {
            name: "tmp".into(),
            rustc_name: "tmp".into(),
            is_library: true,
            edition: "2021".into(),
            roots: vec![lib],
            dependencies: vec![],
            folders: vec![],
            files: vec![],
        };
        let walk = walk_modules(&krate).expect("walk");
        assert_eq!(walk.files.len(), 1, "only the root file");
        assert!(walk.module_paths.contains("crate"));
        assert!(walk.module_paths.contains("crate::absent"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovers_mods_from_cfg_if_and_cfg_star_macros() {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("horizon-macro-mod-{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("lib.rs"),
            r#"
macro_rules! cfg_if {
    ($($tt:tt)*) => {};
}
macro_rules! cfg_fs {
    ($($item:item)*) => { $($item)* };
}

cfg_if! {
    if #[cfg(feature = "net")] {
        mod net;
    } else if #[cfg(feature = "alt")] {
        mod alt;
    } else {
        mod fallback;
    }
}

cfg_fs! {
    pub mod gated;
}

cfg_if! {
    if #[cfg(feature = "missing")] {
        mod absent_file;
    }
}

stringify!(mod must_not_appear;);

mod literal;
"#,
        )
        .unwrap();
        for name in ["net", "alt", "fallback", "gated", "literal"] {
            fs::write(dir.join(format!("{name}.rs")), "pub fn f() {}\n").unwrap();
        }

        let krate = Crate {
            name: "tmp".into(),
            rustc_name: "tmp".into(),
            is_library: true,
            edition: "2021".into(),
            roots: vec![dir.join("lib.rs")],
            dependencies: vec![],
            folders: vec![],
            files: vec![],
        };
        let walk = walk_modules(&krate).expect("walk");
        let paths: HashSet<&str> = walk
            .files
            .iter()
            .filter_map(|f| f.file.path.file_stem().and_then(|s| s.to_str()))
            .collect();

        assert!(paths.contains("lib"));
        assert!(paths.contains("net"), "cfg_if branch union: {paths:?}");
        assert!(paths.contains("alt"), "cfg_if branch union: {paths:?}");
        assert!(paths.contains("fallback"), "cfg_if branch union: {paths:?}");
        assert!(paths.contains("gated"), "cfg_fs! recovery: {paths:?}");
        assert!(paths.contains("literal"));
        assert!(
            !paths.contains("must_not_appear"),
            "stringify! must stay closed: {paths:?}"
        );
        assert!(
            !paths.contains("absent_file"),
            "missing file must be skipped: {paths:?}"
        );
        assert!(walk.module_paths.contains("crate::absent_file"));
        assert!(!walk.module_paths.contains("crate::must_not_appear"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn item_macro_allowlist_matches_expected_names() {
        assert!(is_item_macro_allowlisted("cfg_if"));
        assert!(is_item_macro_allowlisted("cfg_fs"));
        assert!(is_item_macro_allowlisted("cfg_not_rt"));
        assert!(!is_item_macro_allowlisted("stringify"));
        assert!(!is_item_macro_allowlisted("crate_root"));
        assert!(!is_item_macro_allowlisted("cfg")); // builtin predicate macro
    }
}
