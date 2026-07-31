//! Extract per-file facts for the type/method map.
//!
//! Collects free functions (needed so method call sites inside them resolve),
//! type definitions (struct / enum / trait / type alias) with field and alias
//! type paths, inherent `impl Type` methods, doc comments, imports, and call
//! sites — including `receiver.method(...)` forms with one-hop receiver hints
//! and calls hidden inside **allowlisted** macro argument token trees.
//!
//! Macro recovery re-parses each allowlisted macro's token-tree interior as
//! ordinary Rust (arguments of a synthetic call, or array elements for
//! `[…]` macros) and walks the resulting `CallExpr` nodes. That prefers real
//! syntax over raw token scanning. Macros whose arguments are not expression
//! positions (`matches!`, `stringify!`, `cfg!`, `quote!`, …) are never
//! opened. `macro_rules!` / `macro` definition bodies are skipped entirely.
//! When recovery cannot tell whether a site is a real call, it omits it.
//!
//! Trait impl bodies and trait items stay out. Multi-hop receivers
//! (`self.a.b.method()`) are not typed — only certain one-hop hints.
//!
//! # Phase 3
//!
//! Builds the import table from `use` / `pub use` trees (plain paths, brace
//! lists, nested braces, aliases, `self` in a list, globs, relative
//! `crate`/`self`/`super` roots, and external roots), plus `extern crate`
//! / `pub extern crate … as alias` (edition-2015-style crate renames still
//! used as facades, e.g. ripgrep's `pub extern crate grep_cli as cli`).
//! Each leaf binding becomes one [`Import`]. Nested trees are walked via
//! the AST — not by string-splitting the written form.
//!
//! ## Function-body `use` scoping (known limitation)
//!
//! In Rust, a `use` inside a function body is scoped to that body. Extraction
//! still attributes such imports to the enclosing **module**
//! ([`Import::module_path`]). Resolution therefore treats them as module-wide.
//! That widening is a documented limitation, not a separately tracked flag:
//! narrowing body-scoped imports would change resolution outcomes and needs
//! its own design pass. Module-level `use` (including inside inline `mod`
//! blocks) is attributed to that module correctly.

use crate::map::{DocComment, DocCommentKind, Function, FunctionId};
use anyhow::Result;
use ra_ap_syntax::ast::{
    self, AstNode, AstToken, HasGenericArgs, HasName, HasVisibility, LiteralKind, PathSegmentKind,
    VisibilityKind,
};
use ra_ap_syntax::{SourceFile, SyntaxElement, SyntaxKind, SyntaxNode};
use std::collections::{HashMap, HashSet};

/// Facts extracted from one source file, before cross-file resolution.
#[derive(Debug, Clone, Default)]
pub struct FileFacts {
    /// Hex-encoded SHA-256 of the raw file bytes (see [`horizon_map::content_hash`]).
    /// Populated by the pipeline when the file is read from disk; left empty
    /// when facts are built from an in-memory source string alone.
    pub content_hash: String,
    pub functions: Vec<Function>,
    /// Visibility of each function in [`functions`] (same order), for glob
    /// candidate filtering. Not part of the emitted map JSON.
    pub function_visibility: Vec<ItemVisibility>,
    /// Local type definitions in this file (for constructor / assoc exclusion).
    pub types: Vec<TypeDef>,
    pub imports: Vec<Import>,
    /// Call sites not yet attached to a defining function or file.
    pub call_sites: Vec<PendingCall>,
    pub doc_comments: Vec<DocComment>,
    /// Names bound locally inside each free function (`let`, parameters, …).
    /// Used at resolve time to drop closure / binding calls rather than report
    /// them as unresolved free functions. Nested `fn` items are not listed —
    /// those remain real free-function definitions.
    pub local_bindings: HashMap<FunctionId, HashSet<String>>,
    /// Provisional function id → absolutized self-type path for inherent
    /// methods extracted from `impl Type { … }` (no trait). Remapped with
    /// function ids; converted to [`Function::receiver_type`] at map build.
    pub method_receivers: HashMap<FunctionId, String>,
}

/// Kind of type-level item collected during extraction.
///
/// Mirrors [`crate::map::TypeKind`] so the resolve index and the emitted map
/// share vocabulary; kept local so extract stays independent of map serde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Struct,
    Enum,
    Trait,
    TypeAlias,
}

impl TypeKind {
    pub fn to_map(self) -> crate::map::TypeKind {
        match self {
            TypeKind::Struct => crate::map::TypeKind::Struct,
            TypeKind::Enum => crate::map::TypeKind::Enum,
            TypeKind::Trait => crate::map::TypeKind::Trait,
            TypeKind::TypeAlias => crate::map::TypeKind::TypeAlias,
        }
    }
}

/// A type-path mention extracted from a type definition, before resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTypeRef {
    pub type_path: String,
    pub line: u32,
    pub byte_start: u32,
    pub byte_end: u32,
}

/// Visibility of an item, as written (`pub`, `pub(crate)`, …).
///
/// Used for glob candidate sets: a glob only brings in names that would be
/// visible through the globbed module from the importing module. Direct call
/// edges still do not filter on visibility (Phase 2 decision retained).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemVisibility {
    /// `pub`
    Public,
    /// `pub(crate)`
    Crate,
    /// `pub(super)`
    Super,
    /// `pub(self)`
    SelfMod,
    /// `pub(in path)` — path text preserved for best-effort checks.
    InPath(String),
    /// No `pub` — private to the defining module and its descendants.
    Private,
}

/// A type definition visible to the resolver for classifying non-function calls,
/// and the source facts needed to emit a [`crate::map::TypeItem`].
#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    /// Module path containing the type (e.g. `crate::map`), no type-name segment.
    pub module_path: String,
    pub kind: TypeKind,
    /// Variant names when [`TypeKind::Enum`]; empty otherwise.
    pub variants: Vec<String>,
    pub visibility: ItemVisibility,
    /// 1-based line of the item keyword.
    pub line: u32,
    pub byte_start: u32,
    pub byte_end: u32,
    pub doc_comments: Vec<DocComment>,
    /// Field / alias type paths named by this definition (unresolved).
    pub pending_refs: Vec<PendingTypeRef>,
    /// Named struct fields → declared type path (absolutized for locals;
    /// prelude / external roots kept as written). Drives `self.field.method()`
    /// one-hop hints; includes paths [`pending_refs`] omits (e.g. `String`).
    pub fields: Vec<(String, String)>,
}

impl TypeDef {
    /// Full path including the type name (`crate::map::Shape`).
    pub fn full_path(&self) -> String {
        if self.module_path == "crate" {
            format!("crate::{}", self.name)
        } else {
            format!("{}::{}", self.module_path, self.name)
        }
    }
}

/// A single binding produced by expanding a `use` / `pub use` tree.
///
/// Nested brace lists flatten to one [`Import`] per bound name (or glob).
/// Example: `use crate::text::{self, upper as shout}` yields two imports —
/// module path `crate::text` bound as `text`, and `crate::text::upper` bound
/// as `shout`.
///
/// `as _` produces no binding (discarded during extraction).
///
/// See the module-level note on function-body scope widening.
#[derive(Debug, Clone)]
pub struct Import {
    /// Target path with `crate` / `self` / `super` absolutized relative to
    /// [`module_path`] when those roots appear. External roots keep their
    /// crate name (`std::fs`). Glob imports store the module being globbed
    /// (no trailing `*`).
    pub path: String,
    /// Explicit `as` rename, if any. The local binding name is this when
    /// present, otherwise the last path segment (or the globbed module's
    /// last segment is unused — globs have [`is_glob`] set and no local name).
    pub alias: Option<String>,
    pub is_glob: bool,
    /// True when the `use` item has any `pub` visibility (a re-export).
    pub is_public: bool,
    /// Full visibility of the `use` item (controls who can see a re-export).
    pub visibility: ItemVisibility,
    /// Module containing the `use` (inline `mod` segments included).
    ///
    /// Function-body `use` items are attributed here too (module-wide), which
    /// is wider than rustc's real body scope — see the module docs.
    pub module_path: String,
}

impl Import {
    /// Local name bound in the importing module. `None` for globs.
    pub fn local_name(&self) -> Option<&str> {
        if self.is_glob {
            return None;
        }
        if let Some(alias) = &self.alias {
            return Some(alias.as_str());
        }
        self.path.rsplit("::").next()
    }
}

/// Where a pending call will attach after resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOwnerKind {
    /// Inside a free function — attach to that function.
    Function,
    /// Module-level (`const` / `static` init, …) — attach to the [`horizon_map::File`].
    File,
}

/// One-hop hint for a method-call receiver (`x.method(...)`).
///
/// Anything less than certain stays [`MethodReceiverHint::Unknown`] — resolve
/// drops those as associated (never guesses a winner, never invents a Conflict
/// from bare same-named inherent methods elsewhere in the repo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodReceiverHint {
    /// No certain one-hop type (untyped local, parameter without annotation, …).
    Unknown,
    /// Absolutized type path from an annotation, `self`, constructor form, …
    TypePath(String),
    /// `self.field` inside an `impl` — field name; resolve looks up the field's
    /// declared type on the enclosing inherent type.
    SelfField(String),
}

/// A call expression awaiting resolution (pre-map form).
///
/// Distinct from [`horizon_map::CallSite`], which is the post-resolution edge.
#[derive(Debug, Clone)]
pub struct PendingCall {
    /// Path text at the call site (e.g. `shapes::get`, `get`, or `.get` for
    /// a method call).
    pub call_path: String,
    /// 1-based line of the start of the call expression.
    pub line: u32,
    /// Byte offset (UTF-8) of the start of the call expression in the file.
    pub byte_start: u32,
    /// Byte offset (UTF-8) one past the end of the call expression.
    pub byte_end: u32,
    /// Enclosing free-function identity, if [`CallOwnerKind::Function`].
    pub enclosing_function: Option<FunctionId>,
    /// Module path of the call site (e.g. `crate::app`), used for relative
    /// and unqualified resolution. Never includes a function-name segment.
    pub module_path: String,
    pub owner: CallOwnerKind,
    /// Recovered from a macro argument token tree (see module docs).
    pub from_macro: bool,
    /// Set for `MethodCallExpr` sites; `None` for ordinary path-form calls.
    pub method_receiver: Option<MethodReceiverHint>,
}

/// Std/core macros whose arguments are expression positions we are willing to
/// open. Anything else stays opaque — prefer a missed call over a fabricated
/// edge (patterns in `matches!`, tokens in `stringify!`, templates in
/// `quote!`, cfg predicates, …).
const MACRO_EXPR_ALLOWLIST: &[&str] = &[
    "format",
    "print",
    "println",
    "eprint",
    "eprintln",
    "write",
    "writeln",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "vec",
    "dbg",
    "panic",
    "unreachable",
    "unimplemented",
    "todo",
];

/// Bound nested-macro recovery (`println!("{}", format!("{}", f()))`).
const MAX_MACRO_RECOVERY_DEPTH: usize = 8;

/// Whether `name` is an item-pasting macro whose token tree may be opened for
/// `mod` recovery during the module walk.
///
/// - `cfg_if` — the canonical crate; branches are unioned (same spirit as
///   keeping `#[cfg]`-twin function definitions).
/// - `cfg_*` / names starting with `cfg_` — Tokio-style feature-gate item
///   macros (`cfg_fs!`, `cfg_rt!`, `cfg_not_sync!`, …) that paste `$($item:item)*`.
///
/// Macros that do **not** evaluate their contents as items (`stringify!`,
/// `quote!`, `matches!`, user `macro_rules!` bodies, empty-arg expanders like
/// serde's `crate_root!()`) must stay closed. Prefer a missing subtree over a
/// fabricated module tree.
pub fn is_item_macro_allowlisted(name: &str) -> bool {
    name == "cfg_if" || name.starts_with("cfg_")
}

/// Synthetic callee used when re-parsing parenthesised macro args as a call.
const MACRO_PROBE_CALLEE: &str = "__horizon_macro_probe";

/// Extract definitions and call sites from `tree`.
///
/// `crate_key` is the [`FunctionId`] prefix for this compilation unit (see
/// [`horizon_map::Crate::function_id_prefix`]). `file_module_path` is the file's
/// module path from the `mod` walk (`crate`, `crate::shapes`, …). `source` is
/// the file text (for 1-based line mapping). `edition` is the crate edition
/// string (used when re-parsing macro token trees).
///
/// Function ids are provisional (no `#L` suffix yet); call
/// [`assign_function_ids`] over the whole crate before resolving.
pub fn extract_facts(
    tree: &SourceFile,
    source: &str,
    crate_key: &str,
    file_module_path: &str,
    edition: &str,
) -> Result<FileFacts> {
    let lines = LineIndex::new(source);
    let root = tree.syntax();

    let mut raw_defs = Vec::new();
    for node in root.descendants() {
        let Some(func) = ast::Fn::cast(node.clone()) else {
            continue;
        };
        // Trait items and trait-impl methods stay out of the map. Inherent
        // `impl Type { fn … }` methods are in scope for W8.
        if is_trait_or_trait_impl_item(func.syntax()) {
            continue;
        }
        let Some(name) = func.name() else {
            continue;
        };
        let name = name.text().to_string();
        let receiver_type_path = inherent_impl_self_type_path(func.syntax(), file_module_path);
        let module_path = if let Some(ref ty_path) = receiver_type_path {
            if ty_path == "crate" {
                format!("crate::{name}")
            } else {
                format!("{ty_path}::{name}")
            }
        } else {
            function_module_path(func.syntax(), &name, file_module_path)
        };
        let line = func
            .fn_token()
            .map(|t| lines.line_of(u32::from(t.text_range().start())))
            .unwrap_or_else(|| lines.line_of(u32::from(func.syntax().text_range().start())));
        let visibility = visibility_of(&func);
        let doc_comments = extract_outer_docs(func.syntax());
        // Full `ast::Fn` node — attributes, outer docs, signature, body.
        // Not `fn_token()` (drops leading attrs) and not the body alone.
        let range = func.syntax().text_range();
        let byte_start = u32::from(range.start());
        let byte_end = u32::from(range.end());
        raw_defs.push(RawDef {
            name,
            module_path,
            line,
            byte_start,
            byte_end,
            visibility,
            doc_comments,
            syntax: func.syntax().clone(),
            receiver_type_path,
        });
    }

    // Provisional ids (no collision suffix); remapped crate-wide later.
    let mut functions = Vec::with_capacity(raw_defs.len());
    let mut function_visibility = Vec::with_capacity(raw_defs.len());
    let mut method_receivers: HashMap<FunctionId, String> = HashMap::new();
    for def in &raw_defs {
        let id = FunctionId::from_parts(crate_key, &def.module_path, None);
        if let Some(ref ty) = def.receiver_type_path {
            method_receivers.insert(id.clone(), ty.clone());
        }
        functions.push(Function {
            id,
            name: def.name.clone(),
            module_path: def.module_path.clone(),
            receiver_type: None,
            line: def.line,
            byte_start: def.byte_start,
            byte_end: def.byte_end,
            call_sites: Vec::new(),
            doc_comments: def.doc_comments.clone(),
        });
        function_visibility.push(def.visibility.clone());
    }

    let id_by_syntax: HashMap<SyntaxNode, FunctionId> = raw_defs
        .iter()
        .zip(functions.iter())
        .map(|(raw, func)| (raw.syntax.clone(), func.id.clone()))
        .collect();

    let types = extract_types(root, file_module_path, &lines);
    let imports = extract_imports(root, file_module_path);

    let mut local_bindings: HashMap<FunctionId, HashSet<String>> = HashMap::new();
    for (raw, func) in raw_defs.iter().zip(functions.iter()) {
        let names = local_binding_names(&raw.syntax);
        if !names.is_empty() {
            local_bindings.insert(func.id.clone(), names);
        }
    }

    // Declared return types of free functions in this file (name → path), for
    // one-hop `let x = local_fn()` / `let Some(x) = local_fn()` receiver hints.
    let return_types = file_function_return_types(&raw_defs, file_module_path);

    // One-hop receiver types for locals inside each function (annotation /
    // constructor / local-call return forms only — never guessed).
    let mut local_types: HashMap<FunctionId, HashMap<String, String>> = HashMap::new();
    for (raw, func) in raw_defs.iter().zip(functions.iter()) {
        let typed = local_binding_types(&raw.syntax, file_module_path, &return_types);
        if !typed.is_empty() {
            local_types.insert(func.id.clone(), typed);
        }
    }

    let mut call_sites = Vec::new();
    let mut seen_ranges: HashSet<(u32, u32)> = HashSet::new();
    for node in root.descendants() {
        let Some(call) = ast::CallExpr::cast(node.clone()) else {
            continue;
        };
        let Some(pending) = pending_from_call_expr(
            &call,
            &lines,
            file_module_path,
            &id_by_syntax,
            false,
        ) else {
            continue;
        };
        seen_ranges.insert((pending.byte_start, pending.byte_end));
        call_sites.push(pending);
    }

    for node in root.descendants() {
        let Some(call) = ast::MethodCallExpr::cast(node.clone()) else {
            continue;
        };
        let Some(pending) = pending_from_method_call(
            &call,
            &lines,
            file_module_path,
            &id_by_syntax,
            &local_types,
        ) else {
            continue;
        };
        seen_ranges.insert((pending.byte_start, pending.byte_end));
        call_sites.push(pending);
    }

    {
        let mut ctx = MacroRecoveryCtx {
            source,
            edition,
            lines: &lines,
            file_module_path,
            id_by_syntax: &id_by_syntax,
            call_sites: &mut call_sites,
            seen_ranges: &mut seen_ranges,
        };
        recover_macro_calls(root, &mut ctx);
    }

    // Macro recovery appends; restore source order for the emitted map.
    call_sites.sort_by_key(|c| (c.byte_start, c.byte_end));

    Ok(FileFacts {
        content_hash: String::new(),
        functions,
        function_visibility,
        types,
        imports,
        call_sites,
        doc_comments: extract_inner_docs(root),
        local_bindings,
        method_receivers,
    })
}

/// Ident patterns bound in `fn_node`'s own body and parameter list.
///
/// Nested free-function bodies are skipped so each function only owns its
/// own locals. Nested `fn` *names* are definitions, not bindings here.
fn local_binding_names(fn_node: &SyntaxNode) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_local_bindings(fn_node, &mut names, true);
    names
}

fn collect_local_bindings(node: &SyntaxNode, names: &mut HashSet<String>, is_root_fn: bool) {
    if !is_root_fn && node.kind() == SyntaxKind::FN {
        // Nested free function — its params/locals belong to that function.
        return;
    }
    if let Some(ident) = ast::IdentPat::cast(node.clone()) {
        if let Some(name) = ident.name() {
            let text = name.text().to_string();
            if text != "_" {
                names.insert(text);
            }
        }
    }
    for child in node.children() {
        collect_local_bindings(&child, names, false);
    }
}

/// Shared state for allowlisted macro token-tree recovery.
struct MacroRecoveryCtx<'a> {
    source: &'a str,
    edition: &'a str,
    lines: &'a LineIndex,
    file_module_path: &'a str,
    id_by_syntax: &'a HashMap<SyntaxNode, FunctionId>,
    call_sites: &'a mut Vec<PendingCall>,
    seen_ranges: &'a mut HashSet<(u32, u32)>,
}

fn pending_from_call_expr(
    call: &ast::CallExpr,
    lines: &LineIndex,
    file_module_path: &str,
    id_by_syntax: &HashMap<SyntaxNode, FunctionId>,
    from_macro: bool,
) -> Option<PendingCall> {
    let callee = call.expr()?;
    let mut call_path = squish(&callee.syntax().text().to_string());
    if !is_path_like_callee(&call_path) {
        return None;
    }
    if call_path == MACRO_PROBE_CALLEE {
        return None;
    }
    // `Self::method` inside an impl is the impl's type — certain, not a guess.
    if let Some(rest) = call_path.strip_prefix("Self::") {
        if let Some(ty) = enclosing_impl_self_type_path(call.syntax(), file_module_path) {
            call_path = if ty == "crate" {
                format!("crate::{rest}")
            } else {
                format!("{ty}::{rest}")
            };
        }
    }

    let range = call.syntax().text_range();
    let byte_start = u32::from(range.start());
    let byte_end = u32::from(range.end());

    match classify_call_owner(call.syntax()) {
        CallOwner::SkipTraitItem => None,
        CallOwner::Function(fn_node) => {
            let enclosing = id_by_syntax.get(&fn_node).cloned()?;
            Some(PendingCall {
                call_path,
                line: lines.line_of(byte_start),
                byte_start,
                byte_end,
                enclosing_function: Some(enclosing),
                module_path: call_module_path(call.syntax(), file_module_path),
                owner: CallOwnerKind::Function,
                from_macro,
                method_receiver: None,
            })
        }
        CallOwner::ModuleLevel => Some(PendingCall {
            call_path,
            line: lines.line_of(byte_start),
            byte_start,
            byte_end,
            enclosing_function: None,
            module_path: call_module_path(call.syntax(), file_module_path),
            owner: CallOwnerKind::File,
            from_macro,
            method_receiver: None,
        }),
    }
}

/// Open allowlisted `MacroCall` token trees and recover path-form calls.
fn recover_macro_calls(root: &SyntaxNode, ctx: &mut MacroRecoveryCtx<'_>) {
    for node in root.descendants() {
        let Some(mac) = ast::MacroCall::cast(node) else {
            continue;
        };
        if is_inside_macro_definition(mac.syntax()) {
            continue;
        }
        let Some(name) = macro_call_name(&mac) else {
            continue;
        };
        if !MACRO_EXPR_ALLOWLIST.contains(&name.as_str()) {
            continue;
        }
        let Some(tt) = mac.token_tree() else {
            continue;
        };
        let owner_node = mac.syntax();
        let Some(owner_ctx) =
            macro_owner_context(owner_node, ctx.file_module_path, ctx.id_by_syntax)
        else {
            continue;
        };
        let tt_range = tt.syntax().text_range();
        let content_start = u32::from(tt_range.start()).saturating_add(1);
        let content_end = u32::from(tt_range.end()).saturating_sub(1);
        if content_end < content_start || (content_end as usize) > ctx.source.len() {
            continue;
        }
        let open = ctx
            .source
            .as_bytes()
            .get(u32::from(tt_range.start()) as usize)
            .copied();
        let Some(open) = open else {
            continue;
        };
        let content = ctx.source[content_start as usize..content_end as usize].to_string();
        recover_from_macro_content(&content, open, content_start, &owner_ctx, ctx, 0);
    }
}

struct MacroOwnerContext {
    enclosing_function: Option<FunctionId>,
    module_path: String,
    owner: CallOwnerKind,
}

fn macro_owner_context(
    owner_node: &SyntaxNode,
    file_module_path: &str,
    id_by_syntax: &HashMap<SyntaxNode, FunctionId>,
) -> Option<MacroOwnerContext> {
    match classify_call_owner(owner_node) {
        CallOwner::SkipTraitItem => None,
        CallOwner::Function(fn_node) => {
            let enclosing = id_by_syntax.get(&fn_node).cloned()?;
            Some(MacroOwnerContext {
                enclosing_function: Some(enclosing),
                module_path: call_module_path(owner_node, file_module_path),
                owner: CallOwnerKind::Function,
            })
        }
        CallOwner::ModuleLevel => Some(MacroOwnerContext {
            enclosing_function: None,
            module_path: call_module_path(owner_node, file_module_path),
            owner: CallOwnerKind::File,
        }),
    }
}

pub(crate) fn macro_call_name(mac: &ast::MacroCall) -> Option<String> {
    let path = mac.path()?;
    let segs = path_segments(&path);
    segs.last().cloned()
}

pub(crate) fn is_inside_macro_definition(node: &SyntaxNode) -> bool {
    for ancestor in node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::MACRO_RULES | SyntaxKind::MACRO_DEF => return true,
            _ => {}
        }
    }
    false
}

/// Re-parse `content` (token-tree interior) and collect path-form [`CallExpr`]s,
/// mapping their ranges back into the original file via `content_start`.
fn recover_from_macro_content(
    content: &str,
    open_delim: u8,
    content_start: u32,
    owner_ctx: &MacroOwnerContext,
    ctx: &mut MacroRecoveryCtx<'_>,
    depth: usize,
) {
    if depth > MAX_MACRO_RECOVERY_DEPTH || content.is_empty() {
        return;
    }

    let (wrapped, wrapped_content_start) = match open_delim {
        b'(' => {
            let prefix = format!("fn __horizon_probe() {{ {MACRO_PROBE_CALLEE}(");
            let wrapped = format!("{prefix}{content}); }}");
            (wrapped, prefix.len() as u32)
        }
        b'[' => {
            let prefix = "fn __horizon_probe() { let _ = [";
            let wrapped = format!("{prefix}{content}]; }}");
            (wrapped, prefix.len() as u32)
        }
        b'{' => {
            let prefix = "fn __horizon_probe() { { ";
            let wrapped = format!("{prefix}{content} }} }}");
            (wrapped, prefix.len() as u32)
        }
        _ => return,
    };

    let Ok(tree) = horizon_engine::parse::parse_source(&wrapped, &ctx.edition.to_string()) else {
        return;
    };
    let root = tree.syntax();

    for node in root.descendants() {
        if let Some(call) = ast::CallExpr::cast(node.clone()) {
            let Some(callee) = call.expr() else {
                continue;
            };
            let call_path = squish(&callee.syntax().text().to_string());
            if call_path == MACRO_PROBE_CALLEE || !is_path_like_callee(&call_path) {
                continue;
            }
            let range = call.syntax().text_range();
            let wrapped_start = u32::from(range.start());
            let wrapped_end = u32::from(range.end());
            let Some(byte_start) =
                map_wrapped_offset(wrapped_start, wrapped_content_start, content_start)
            else {
                continue;
            };
            let Some(byte_end) =
                map_wrapped_offset(wrapped_end, wrapped_content_start, content_start)
            else {
                continue;
            };
            if byte_end < byte_start {
                continue;
            }
            // Require the mapped span to match the reparsed call text in the
            // original token-tree content — drops phantoms from a bad reparse.
            let call_text = call.syntax().text().to_string();
            let rel_start = (byte_start - content_start) as usize;
            let rel_end = (byte_end - content_start) as usize;
            let Some(orig_slice) = content.get(rel_start..rel_end) else {
                continue;
            };
            if orig_slice != call_text.as_str()
                && squish(orig_slice) != squish(&call_text)
            {
                continue;
            }
            if !ctx.seen_ranges.insert((byte_start, byte_end)) {
                continue;
            }
            ctx.call_sites.push(PendingCall {
                call_path,
                line: ctx.lines.line_of(byte_start),
                byte_start,
                byte_end,
                enclosing_function: owner_ctx.enclosing_function.clone(),
                module_path: owner_ctx.module_path.clone(),
                owner: owner_ctx.owner,
                from_macro: true,
                method_receiver: None,
            });
            continue;
        }

        if let Some(mac) = ast::MacroCall::cast(node) {
            let Some(name) = macro_call_name(&mac) else {
                continue;
            };
            if !MACRO_EXPR_ALLOWLIST.contains(&name.as_str()) {
                continue;
            }
            let Some(tt) = mac.token_tree() else {
                continue;
            };
            let tt_range = tt.syntax().text_range();
            let nested_wrapped_start = u32::from(tt_range.start()).saturating_add(1);
            let nested_wrapped_end = u32::from(tt_range.end()).saturating_sub(1);
            let Some(nested_orig_start) = map_wrapped_offset(
                nested_wrapped_start,
                wrapped_content_start,
                content_start,
            ) else {
                continue;
            };
            let Some(nested_orig_end) =
                map_wrapped_offset(nested_wrapped_end, wrapped_content_start, content_start)
            else {
                continue;
            };
            if nested_orig_end < nested_orig_start {
                continue;
            }
            let rel_s = (nested_orig_start - content_start) as usize;
            let rel_e = (nested_orig_end - content_start) as usize;
            let Some(nested_content) = content.get(rel_s..rel_e).map(str::to_string) else {
                continue;
            };
            let open = wrapped
                .as_bytes()
                .get(u32::from(tt_range.start()) as usize)
                .copied();
            let Some(open) = open else {
                continue;
            };
            recover_from_macro_content(
                &nested_content,
                open,
                nested_orig_start,
                owner_ctx,
                ctx,
                depth + 1,
            );
        }
    }
}

fn map_wrapped_offset(
    wrapped_offset: u32,
    wrapped_content_start: u32,
    original_content_start: u32,
) -> Option<u32> {
    if wrapped_offset < wrapped_content_start {
        return None;
    }
    Some(original_content_start + (wrapped_offset - wrapped_content_start))
}

/// Outer docs attached to an item (`///`, `/** … */`, `#[doc = "…"]`).
///
/// `ra_ap_syntax` keeps doc comments and attributes as children of the item,
/// so an intervening `#[inline]` between `///` and `fn` does not break the
/// association. Consecutive pieces are joined into one [`DocComment`].
fn extract_outer_docs(node: &SyntaxNode) -> Vec<DocComment> {
    let pieces = collect_doc_pieces(node, DocCommentKind::Outer);
    join_doc_pieces(DocCommentKind::Outer, pieces)
}

/// Inner docs for a file / module (`//!`, `/*! … */`, `#![doc = "…"]`).
fn extract_inner_docs(node: &SyntaxNode) -> Vec<DocComment> {
    let pieces = collect_doc_pieces(node, DocCommentKind::Inner);
    join_doc_pieces(DocCommentKind::Inner, pieces)
}

fn join_doc_pieces(kind: DocCommentKind, pieces: Vec<String>) -> Vec<DocComment> {
    if pieces.is_empty() {
        return Vec::new();
    }
    vec![DocComment {
        kind,
        text: pieces.join("\n"),
    }]
}

/// Walk direct children of `node` in source order, collecting doc comments and
/// `doc = "…"` attributes of the requested kind. Ordinary `//` / `/* */` are
/// skipped (`Comment::is_doc` is false for them).
fn collect_doc_pieces(node: &SyntaxNode, want: DocCommentKind) -> Vec<String> {
    let mut pieces = Vec::new();
    for element in node.children_with_tokens() {
        match element {
            SyntaxElement::Token(token) => {
                let Some(comment) = ast::Comment::cast(token) else {
                    continue;
                };
                if !comment.is_doc() {
                    continue;
                }
                let kind = if comment.is_inner() {
                    DocCommentKind::Inner
                } else {
                    DocCommentKind::Outer
                };
                if kind != want {
                    continue;
                }
                let Some((raw, _)) = comment.doc_comment() else {
                    continue;
                };
                pieces.push(normalize_doc_text(raw));
            }
            SyntaxElement::Node(child) => {
                let Some(attr) = ast::Attr::cast(child) else {
                    continue;
                };
                let kind = if attr.kind().is_inner() {
                    DocCommentKind::Inner
                } else {
                    DocCommentKind::Outer
                };
                if kind != want {
                    continue;
                }
                if let Some(raw) = doc_attr_string(&attr) {
                    pieces.push(normalize_doc_text(&raw));
                }
            }
        }
    }
    pieces
}

/// Body of `#[doc = "…"]` / `#![doc = "…"]`. Other `doc(…)` forms (e.g.
/// `#[doc(hidden)]`) are not documentation text and are ignored.
fn doc_attr_string(attr: &ast::Attr) -> Option<String> {
    let meta = attr.meta()?;
    let ast::Meta::KeyValueMeta(kv) = meta else {
        return None;
    };
    let path = kv.path()?;
    let name = path.as_single_name_ref()?;
    if name.text() != "doc" {
        return None;
    }
    let expr = kv.expr()?;
    let ast::Expr::Literal(lit) = expr else {
        return None;
    };
    match lit.kind() {
        LiteralKind::String(s) => s.value().ok().map(|cow| cow.into_owned()),
        _ => None,
    }
}

/// Strip the conventional single leading space after `///` / `//!` on each
/// line; keep further indentation (needed for fenced code blocks in docs).
fn normalize_doc_text(raw: &str) -> String {
    raw.lines()
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Collect struct / enum / trait / type-alias definitions (and enum variants).
///
/// Items inside `impl` / `trait` bodies are skipped — associated types are not
/// free type definitions for our purposes.
fn extract_types(root: &SyntaxNode, file_module_path: &str, lines: &LineIndex) -> Vec<TypeDef> {
    let mut types = Vec::new();
    for node in root.descendants() {
        if is_inside_impl_or_trait(&node) {
            continue;
        }
        if let Some(en) = ast::Enum::cast(node.clone()) {
            let Some(name) = en.name() else {
                continue;
            };
            let variants = en
                .variant_list()
                .map(|list| {
                    list.variants()
                        .filter_map(|v| v.name().map(|n| n.text().to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let module_path = item_module_path(en.syntax(), file_module_path);
            let range = en.syntax().text_range();
            let mut pending_refs = Vec::new();
            if let Some(list) = en.variant_list() {
                for variant in list.variants() {
                    collect_field_type_refs(variant.field_list(), lines, &mut pending_refs);
                }
            }
            types.push(TypeDef {
                name: name.text().to_string(),
                module_path,
                kind: TypeKind::Enum,
                variants,
                visibility: visibility_of(&en),
                line: lines.line_of(u32::from(range.start())),
                byte_start: u32::from(range.start()),
                byte_end: u32::from(range.end()),
                doc_comments: extract_outer_docs(en.syntax()),
                pending_refs,
                fields: Vec::new(),
            });
            continue;
        }
        if let Some(st) = ast::Struct::cast(node.clone()) {
            let Some(name) = st.name() else {
                continue;
            };
            let module_path = item_module_path(st.syntax(), file_module_path);
            let range = st.syntax().text_range();
            let mut pending_refs = Vec::new();
            collect_field_type_refs(st.field_list(), lines, &mut pending_refs);
            let fields = collect_named_fields(st.field_list(), file_module_path);
            types.push(TypeDef {
                name: name.text().to_string(),
                module_path,
                kind: TypeKind::Struct,
                variants: Vec::new(),
                visibility: visibility_of(&st),
                line: lines.line_of(u32::from(range.start())),
                byte_start: u32::from(range.start()),
                byte_end: u32::from(range.end()),
                doc_comments: extract_outer_docs(st.syntax()),
                pending_refs,
                fields,
            });
            continue;
        }
        if let Some(tr) = ast::Trait::cast(node.clone()) {
            let Some(name) = tr.name() else {
                continue;
            };
            let module_path = item_module_path(tr.syntax(), file_module_path);
            let range = tr.syntax().text_range();
            types.push(TypeDef {
                name: name.text().to_string(),
                module_path,
                kind: TypeKind::Trait,
                variants: Vec::new(),
                visibility: visibility_of(&tr),
                line: lines.line_of(u32::from(range.start())),
                byte_start: u32::from(range.start()),
                byte_end: u32::from(range.end()),
                doc_comments: extract_outer_docs(tr.syntax()),
                pending_refs: Vec::new(),
                fields: Vec::new(),
            });
            continue;
        }
        if let Some(ta) = ast::TypeAlias::cast(node.clone()) {
            let Some(name) = ta.name() else {
                continue;
            };
            let module_path = item_module_path(ta.syntax(), file_module_path);
            let range = ta.syntax().text_range();
            let mut pending_refs = Vec::new();
            if let Some(ty) = ta.ty() {
                collect_type_path_refs(&ty, lines, &mut pending_refs);
            }
            types.push(TypeDef {
                name: name.text().to_string(),
                module_path,
                kind: TypeKind::TypeAlias,
                variants: Vec::new(),
                visibility: visibility_of(&ta),
                line: lines.line_of(u32::from(range.start())),
                byte_start: u32::from(range.start()),
                byte_end: u32::from(range.end()),
                doc_comments: extract_outer_docs(ta.syntax()),
                pending_refs,
                fields: Vec::new(),
            });
        }
    }
    types
}

fn collect_field_type_refs(
    fields: Option<ast::FieldList>,
    lines: &LineIndex,
    out: &mut Vec<PendingTypeRef>,
) {
    let Some(fields) = fields else {
        return;
    };
    match fields {
        ast::FieldList::RecordFieldList(list) => {
            for field in list.fields() {
                if let Some(ty) = field.ty() {
                    collect_type_path_refs(&ty, lines, out);
                }
            }
        }
        ast::FieldList::TupleFieldList(list) => {
            for field in list.fields() {
                if let Some(ty) = field.ty() {
                    collect_type_path_refs(&ty, lines, out);
                }
            }
        }
    }
}

/// Named record fields with declared type paths (including prelude names).
fn collect_named_fields(
    fields: Option<ast::FieldList>,
    file_module_path: &str,
) -> Vec<(String, String)> {
    let Some(ast::FieldList::RecordFieldList(list)) = fields else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for field in list.fields() {
        let Some(name) = field.name() else {
            continue;
        };
        let Some(ty) = field.ty() else {
            continue;
        };
        if let Some(path) = field_type_path(&ty, file_module_path) {
            out.push((name.text().to_string(), path));
        }
    }
    out
}

/// Declared field type path for one-hop `self.field` hints.
///
/// Unlike [`simple_type_path`], prelude names (`String`, `Vec`, …) are kept so
/// a method on an external/prelude field can be dropped rather than matched
/// against unrelated local inherent methods that share the name.
fn field_type_path(ty: &ast::Type, file_module_path: &str) -> Option<String> {
    let mut cur = ty.clone();
    for _ in 0..3 {
        if let Some(ref_ty) = ast::RefType::cast(cur.syntax().clone()) {
            cur = ref_ty.ty()?;
            continue;
        }
        if let Some(paren) = ast::ParenType::cast(cur.syntax().clone()) {
            cur = paren.ty()?;
            continue;
        }
        break;
    }
    let path_ty = ast::PathType::cast(cur.syntax().clone())?;
    let path = path_ty.path()?;
    let segs = path_segments(&path);
    if segs.is_empty() {
        return None;
    }
    if segs.len() == 1 {
        let s = segs[0].as_str();
        if is_primitive_type_name(s) || is_prelude_type_name(s) {
            return Some(segs[0].clone());
        }
        if s == "Self" || s == "_" {
            return None;
        }
    }
    Some(absolutize_local_type_path(&segs, file_module_path))
}

/// Collect path-form type mentions under `ty` (skipping primitives / `Self`).
fn collect_type_path_refs(ty: &ast::Type, lines: &LineIndex, out: &mut Vec<PendingTypeRef>) {
    for node in ty.syntax().descendants() {
        let Some(path_ty) = ast::PathType::cast(node) else {
            continue;
        };
        let Some(path) = path_ty.path() else {
            continue;
        };
        let segs = path_segments(&path);
        if segs.is_empty() {
            continue;
        }
        // Drop language / prelude / placeholder forms — not indexed map types.
        if segs.len() == 1 {
            let s = segs[0].as_str();
            if is_primitive_type_name(s) || is_prelude_type_name(s) || s == "Self" || s == "_" {
                continue;
            }
        }
        let type_path = segs.join("::");
        if !is_path_like_callee(&type_path) {
            continue;
        }
        let range = path_ty.syntax().text_range();
        out.push(PendingTypeRef {
            type_path,
            line: lines.line_of(u32::from(range.start())),
            byte_start: u32::from(range.start()),
            byte_end: u32::from(range.end()),
        });
    }
}

fn is_primitive_type_name(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "char"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
    )
}

fn is_prelude_type_name(name: &str) -> bool {
    matches!(
        name,
        "Option"
            | "Result"
            | "Vec"
            | "String"
            | "Box"
            | "Rc"
            | "Arc"
            | "Cell"
            | "RefCell"
            | "Pin"
            | "Path"
            | "PathBuf"
            | "OsStr"
            | "OsString"
            | "CString"
            | "CStr"
            | "Cow"
            | "HashMap"
            | "HashSet"
            | "BTreeMap"
            | "BTreeSet"
            | "VecDeque"
            | "LinkedList"
            | "BinaryHeap"
            | "Duration"
            | "Instant"
            | "Ok"
            | "Err"
            | "Some"
            | "None"
    )
}

/// Expand every `use` / `pub use` tree and `extern crate` item into
/// flattened [`Import`]s.
///
/// `pub extern crate dep as name` is recorded like `pub use dep as name`: a
/// public binding of the dependency crate under `name`. That is the facade
/// form ripgrep's `grep` crate uses (`pub extern crate grep_cli as cli`).
fn extract_imports(root: &SyntaxNode, file_module_path: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for node in root.descendants() {
        if let Some(use_item) = ast::Use::cast(node.clone()) {
            // Nested `use` trees are walked from the top-level Use item only.
            // Descendant UseTree nodes must not be double-scanned as Use items.
            let Some(tree) = use_item.use_tree() else {
                continue;
            };
            let module_path = call_module_path(use_item.syntax(), file_module_path);
            let visibility = visibility_of(&use_item);
            let is_public = !matches!(visibility, ItemVisibility::Private);
            flatten_use_tree(
                &tree,
                &[],
                &module_path,
                &visibility,
                is_public,
                &mut imports,
            );
            continue;
        }
        let Some(ext) = ast::ExternCrate::cast(node.clone()) else {
            continue;
        };
        let Some(name_ref) = ext.name_ref() else {
            continue;
        };
        let crate_name = name_ref.text().to_string();
        let rename = ext.rename();
        if rename.as_ref().is_some_and(|r| r.underscore_token().is_some()) {
            continue;
        }
        let alias = rename.and_then(|r| r.name().map(|n| n.text().to_string()));
        let module_path = call_module_path(ext.syntax(), file_module_path);
        let visibility = visibility_of(&ext);
        let is_public = !matches!(visibility, ItemVisibility::Private);
        imports.push(Import {
            path: crate_name,
            alias,
            is_glob: false,
            is_public,
            visibility,
            module_path,
        });
    }
    imports
}

/// Recursively flatten a `UseTree` into leaf bindings.
///
/// `prefix` holds path segments accumulated from outer trees
/// (`use crate::{text::{upper}}` → prefix `[crate]` then `[crate, text]`).
fn flatten_use_tree(
    tree: &ast::UseTree,
    prefix: &[String],
    module_path: &str,
    visibility: &ItemVisibility,
    is_public: bool,
    out: &mut Vec<Import>,
) {
    let mut path_segs = prefix.to_vec();
    if let Some(path) = tree.path() {
        path_segs.extend(path_segments(&path));
    }

    if let Some(list) = tree.use_tree_list() {
        for child in list.use_trees() {
            flatten_use_tree(
                &child,
                &path_segs,
                module_path,
                visibility,
                is_public,
                out,
            );
        }
        return;
    }

    if tree.star_token().is_some() {
        let abs = absolutize_path_segments(&path_segs, module_path);
        out.push(Import {
            path: abs,
            alias: None,
            is_glob: true,
            is_public,
            visibility: visibility.clone(),
            module_path: module_path.to_string(),
        });
        return;
    }

    // `self` alone in a brace list: `use crate::text::{self}` — bind the
    // prefix module under its last segment name.
    if path_segs.last().is_some_and(|s| s == "self") && path_segs.len() > 1 {
        path_segs.pop();
    } else if path_segs.len() == 1 && path_segs[0] == "self" {
        // `use self::foo` keeps `self`; bare `{self}` at crate root is rare.
        // If the only segment is `self` with an empty prefix, bind the module.
        path_segs.clear();
        // Represent "this module" as the importing module path's segments.
        path_segs.extend(module_path_segments(module_path));
    }

    if path_segs.is_empty() {
        return;
    }

    let rename = tree.rename();
    if rename.as_ref().is_some_and(|r| r.underscore_token().is_some()) {
        // `as _` — intentionally binds nothing.
        return;
    }
    let alias = rename.and_then(|r| r.name().map(|n| n.text().to_string()));
    let abs = absolutize_path_segments(&path_segs, module_path);
    out.push(Import {
        path: abs,
        alias,
        is_glob: false,
        is_public,
        visibility: visibility.clone(),
        module_path: module_path.to_string(),
    });
}

fn path_segments(path: &ast::Path) -> Vec<String> {
    path.segments()
        .filter_map(|seg| match seg.kind()? {
            PathSegmentKind::Name(name) => Some(name.text().to_string()),
            PathSegmentKind::SelfKw => Some("self".into()),
            PathSegmentKind::SuperKw => Some("super".into()),
            PathSegmentKind::CrateKw => Some("crate".into()),
            PathSegmentKind::SelfTypeKw => Some("Self".into()),
            PathSegmentKind::Type { .. } => None,
        })
        .collect()
}

fn module_path_segments(module_path: &str) -> Vec<String> {
    module_path.split("::").map(str::to_string).collect()
}

/// Resolve leading `crate` / `self` / `super` relative to `from_module`.
///
/// Paths that start with any other segment (e.g. `std`, `serde`) are kept as
/// written — they name an external crate or an as-yet-unresolved root.
/// Absolutize `crate` / `self` / `super` path segments relative to `from_module`.
pub fn absolutize_type_path(segments: &[&str], from_module: &str) -> String {
    let owned: Vec<String> = segments.iter().map(|s| (*s).to_string()).collect();
    absolutize_path_segments(&owned, from_module)
}

fn absolutize_path_segments(segments: &[String], from_module: &str) -> String {
    if segments.is_empty() {
        return from_module.to_string();
    }
    let mut module = from_module.to_string();
    let mut i = 0usize;
    let starts_with_kw = matches!(segments[0].as_str(), "crate" | "self" | "super");
    if starts_with_kw {
        while i < segments.len() {
            match segments[i].as_str() {
                "crate" => {
                    module = "crate".to_string();
                    i += 1;
                }
                "self" => {
                    i += 1;
                }
                "super" => {
                    match parent_module_path(&module) {
                        Some(p) => module = p,
                        None => break,
                    }
                    i += 1;
                }
                _ => break,
            }
        }
        let rest = &segments[i..];
        if rest.is_empty() {
            return module;
        }
        return join_path(&module, rest);
    }
    segments.join("::")
}

fn parent_module_path(module: &str) -> Option<String> {
    if module == "crate" {
        None
    } else {
        module.rsplit_once("::").map(|(p, _)| p.to_string())
    }
}

fn visibility_of(node: &impl HasVisibility) -> ItemVisibility {
    match node.visibility() {
        None => ItemVisibility::Private,
        Some(v) => match v.kind() {
            VisibilityKind::Pub => ItemVisibility::Public,
            VisibilityKind::PubCrate => ItemVisibility::Crate,
            VisibilityKind::PubSuper => ItemVisibility::Super,
            VisibilityKind::PubSelf => ItemVisibility::SelfMod,
            VisibilityKind::In(path) => {
                let segs = path_segments(&path);
                ItemVisibility::InPath(segs.join("::"))
            }
        },
    }
}

fn is_inside_impl_or_trait(node: &SyntaxNode) -> bool {
    for ancestor in node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::IMPL | SyntaxKind::TRAIT => return true,
            _ => {}
        }
    }
    false
}

/// Module path of a type / item (inline `mod` segments, no item-name segment).
fn item_module_path(node: &SyntaxNode, file_module_path: &str) -> String {
    call_module_path(node, file_module_path)
}

/// Assign [`crate::map::TypeId`]s after all type definitions in a crate are known.
///
/// Returns `(items, provisional_full_path → TypeId)` so type refs can be
/// resolved against the same ids the map emits.
pub fn assign_type_ids(crate_key: &str, types: &[TypeDef]) -> Vec<crate::map::TypeItem> {
    let mut path_counts: HashMap<String, usize> = HashMap::new();
    for ty in types {
        *path_counts.entry(ty.full_path()).or_default() += 1;
    }

    types
        .iter()
        .map(|ty| {
            let full = ty.full_path();
            let line = if path_counts.get(&full).copied().unwrap_or(0) > 1 {
                Some(ty.line)
            } else {
                None
            };
            crate::map::TypeItem {
                id: crate::map::TypeId::from_parts(crate_key, &full, line),
                name: ty.name.clone(),
                kind: ty.kind.to_map(),
                module_path: full,
                line: ty.line,
                byte_start: ty.byte_start,
                byte_end: ty.byte_end,
                variants: ty.variants.clone(),
                type_refs: Vec::new(),
                doc_comments: ty.doc_comments.clone(),
            }
        })
        .collect()
}

/// Assign [`FunctionId`]s after all definitions in a crate are known.
///
/// Colliding `module_path` values (typically `#[cfg]` duplicates) all receive
/// a `#L{line}` suffix. Updates `functions` in place and returns a map from
/// provisional id → final id so pending calls can be remapped.
pub fn assign_function_ids(
    crate_key: &str,
    functions: &mut [Function],
) -> HashMap<FunctionId, FunctionId> {
    let mut path_counts: HashMap<String, usize> = HashMap::new();
    for func in functions.iter() {
        *path_counts
            .entry(func.module_path.clone())
            .or_default() += 1;
    }

    let mut remap = HashMap::new();
    for func in functions.iter_mut() {
        let old = func.id.clone();
        let line = if path_counts.get(&func.module_path).copied().unwrap_or(0) > 1 {
            Some(func.line)
        } else {
            None
        };
        let new_id = FunctionId::from_parts(crate_key, &func.module_path, line);
        remap.insert(old, new_id.clone());
        func.id = new_id;
    }
    remap
}

/// Remap [`PendingCall::enclosing_function`] after crate-wide id assignment.
pub fn remap_pending_calls(calls: &mut [PendingCall], remap: &HashMap<FunctionId, FunctionId>) {
    for call in calls {
        if let Some(old) = call.enclosing_function.take() {
            call.enclosing_function = Some(remap.get(&old).cloned().unwrap_or(old));
        }
    }
}

/// Remap [`FileFacts::local_bindings`] keys after crate-wide id assignment.
pub fn remap_local_bindings(
    bindings: &mut HashMap<FunctionId, HashSet<String>>,
    remap: &HashMap<FunctionId, FunctionId>,
) {
    let old = std::mem::take(bindings);
    for (id, names) in old {
        let new_id = remap.get(&id).cloned().unwrap_or(id);
        bindings.insert(new_id, names);
    }
}

struct RawDef {
    name: String,
    module_path: String,
    line: u32,
    byte_start: u32,
    byte_end: u32,
    visibility: ItemVisibility,
    doc_comments: Vec<DocComment>,
    syntax: SyntaxNode,
    /// Absolutized self-type path when this is an inherent method.
    receiver_type_path: Option<String>,
}

enum CallOwner {
    /// Free function or inherent method (in the extract index).
    Function(SyntaxNode),
    ModuleLevel,
    /// Trait item or trait-impl method — still out of map scope.
    SkipTraitItem,
}

fn classify_call_owner(node: &SyntaxNode) -> CallOwner {
    for ancestor in node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::FN => {
                if is_trait_or_trait_impl_item(&ancestor) {
                    return CallOwner::SkipTraitItem;
                }
                return CallOwner::Function(ancestor);
            }
            SyntaxKind::TRAIT => {
                return CallOwner::SkipTraitItem;
            }
            SyntaxKind::IMPL => {
                // Inside an impl but outside any fn — module-level relative to
                // the impl (const items). Trait impls stay skipped.
                if let Some(impl_) = ast::Impl::cast(ancestor.clone()) {
                    if impl_.trait_().is_some() {
                        return CallOwner::SkipTraitItem;
                    }
                }
            }
            _ => {}
        }
    }
    CallOwner::ModuleLevel
}

/// Full module path including the function name, rooted at `crate`.
///
/// Starts from the file's module path (from the `mod` walk), then adds inline
/// `mod` segments and enclosing free-function names for nested `fn`s.
fn function_module_path(fn_node: &SyntaxNode, name: &str, file_module_path: &str) -> String {
    let mut parts = Vec::new();
    for ancestor in fn_node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::MODULE => {
                if let Some(n) = ast::Module::cast(ancestor).and_then(|m| m.name()) {
                    parts.push(n.text().to_string());
                }
            }
            SyntaxKind::FN => {
                if let Some(n) = ast::Fn::cast(ancestor).and_then(|f| f.name()) {
                    parts.push(n.text().to_string());
                }
            }
            _ => {}
        }
    }
    parts.reverse();
    parts.push(name.to_string());
    join_path(file_module_path, &parts)
}

/// Module path of a call site (no function-name segment).
fn call_module_path(node: &SyntaxNode, file_module_path: &str) -> String {
    let mut parts = Vec::new();
    for ancestor in node.ancestors().skip(1) {
        if ancestor.kind() == SyntaxKind::MODULE {
            if let Some(n) = ast::Module::cast(ancestor).and_then(|m| m.name()) {
                parts.push(n.text().to_string());
            }
        }
    }
    parts.reverse();
    if parts.is_empty() {
        file_module_path.to_string()
    } else {
        join_path(file_module_path, &parts)
    }
}

fn join_path(file_module_path: &str, parts: &[String]) -> String {
    if parts.is_empty() {
        return file_module_path.to_string();
    }
    if file_module_path == "crate" {
        format!("crate::{}", parts.join("::"))
    } else {
        format!("{file_module_path}::{}", parts.join("::"))
    }
}

/// Trait items and `impl Trait for Type` methods — still out of the map.
fn is_trait_or_trait_impl_item(fn_node: &SyntaxNode) -> bool {
    for ancestor in fn_node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::TRAIT => return true,
            SyntaxKind::IMPL => {
                if let Some(impl_) = ast::Impl::cast(ancestor) {
                    return impl_.trait_().is_some();
                }
                return true;
            }
            SyntaxKind::FN => return false,
            _ => {}
        }
    }
    false
}

/// Absolutized self-type path for an inherent `impl Type { … }` method.
fn inherent_impl_self_type_path(fn_node: &SyntaxNode, file_module_path: &str) -> Option<String> {
    for ancestor in fn_node.ancestors().skip(1) {
        if ancestor.kind() == SyntaxKind::FN {
            return None;
        }
        let Some(impl_) = ast::Impl::cast(ancestor) else {
            continue;
        };
        if impl_.trait_().is_some() {
            return None;
        }
        return impl_self_type_path(&impl_, file_module_path);
    }
    None
}

/// Absolutized `Self` type of the `impl` lexically enclosing `node`.
///
/// Accepts inherent and trait impls — the receiver type is still `Type` in
/// `impl Trait for Type`. Trait *methods* stay out of the map via
/// [`is_trait_or_trait_impl_item`]; this is only a certain receiver hint.
fn enclosing_impl_self_type_path(node: &SyntaxNode, file_module_path: &str) -> Option<String> {
    for ancestor in node.ancestors().skip(1) {
        let Some(impl_) = ast::Impl::cast(ancestor) else {
            continue;
        };
        return impl_self_type_path(&impl_, file_module_path);
    }
    None
}

fn impl_self_type_path(impl_: &ast::Impl, file_module_path: &str) -> Option<String> {
    let ty = impl_.self_ty()?;
    let path_ty = ast::PathType::cast(ty.syntax().clone())?;
    let path = path_ty.path()?;
    let segs = path_segments(&path);
    if segs.is_empty() {
        return None;
    }
    // Only simple path types (no dyn/impl Trait receivers as self type).
    // Bare `Cache` is relative to the file module; `crate::…` / `super::…`
    // go through the shared absolutizer. (That helper leaves unknown bare
    // roots untouched for import paths like `serde` — not appropriate here.)
    Some(absolutize_local_type_path(&segs, file_module_path))
}

fn absolutize_local_type_path(segments: &[String], from_module: &str) -> String {
    if segments.is_empty() {
        return from_module.to_string();
    }
    if matches!(segments[0].as_str(), "crate" | "self" | "super") {
        absolutize_path_segments(segments, from_module)
    } else {
        join_path(from_module, segments)
    }
}

fn pending_from_method_call(
    call: &ast::MethodCallExpr,
    lines: &LineIndex,
    file_module_path: &str,
    id_by_syntax: &HashMap<SyntaxNode, FunctionId>,
    local_types: &HashMap<FunctionId, HashMap<String, String>>,
) -> Option<PendingCall> {
    let name = call.name_ref()?.text().to_string();
    if !is_ident(&name) {
        return None;
    }
    let range = call.syntax().text_range();
    let byte_start = u32::from(range.start());
    let byte_end = u32::from(range.end());
    let call_path = format!(".{name}");

    let (owner, enclosing) = match classify_call_owner(call.syntax()) {
        CallOwner::SkipTraitItem => return None,
        CallOwner::Function(fn_node) => {
            let enclosing = id_by_syntax.get(&fn_node).cloned()?;
            (CallOwnerKind::Function, Some(enclosing))
        }
        CallOwner::ModuleLevel => (CallOwnerKind::File, None),
    };

    let hint = match enclosing.as_ref() {
        Some(fid) => infer_receiver_hint(call, local_types.get(fid), file_module_path),
        None => MethodReceiverHint::Unknown,
    };

    Some(PendingCall {
        call_path,
        line: lines.line_of(byte_start),
        byte_start,
        byte_end,
        enclosing_function: enclosing,
        module_path: call_module_path(call.syntax(), file_module_path),
        owner,
        from_macro: false,
        method_receiver: Some(hint),
    })
}

/// One-hop receiver type: typed `let` / parameter, bare `self`, `self.field`,
/// or constructor / associated call form `Type { … }` / `Type(…)` / `Type::assoc(…)`.
fn infer_receiver_hint(
    call: &ast::MethodCallExpr,
    local_types: Option<&HashMap<String, String>>,
    file_module_path: &str,
) -> MethodReceiverHint {
    let Some(receiver) = call.receiver() else {
        return MethodReceiverHint::Unknown;
    };
    // Strip a shallow layer of `&` / `*` / parens.
    let mut expr = receiver;
    for _ in 0..3 {
        if let Some(ref_expr) = ast::RefExpr::cast(expr.syntax().clone()) {
            if let Some(inner) = ref_expr.expr() {
                expr = inner;
                continue;
            }
        }
        if let Some(prefix) = ast::PrefixExpr::cast(expr.syntax().clone()) {
            if let Some(inner) = prefix.expr() {
                expr = inner;
                continue;
            }
        }
        if let Some(paren) = ast::ParenExpr::cast(expr.syntax().clone()) {
            if let Some(inner) = paren.expr() {
                expr = inner;
                continue;
            }
        }
        break;
    }

    // Bare `self` inside an impl — the impl's Self type is certain.
    // Not `self.clone()`, `&self` after peel is still path `self`.
    if let Some(path_expr) = ast::PathExpr::cast(expr.syntax().clone()) {
        if let Some(path) = path_expr.path() {
            let segs = path_segments(&path);
            if segs.len() == 1 && segs[0] == "self" {
                if let Some(ty) = enclosing_impl_self_type_path(call.syntax(), file_module_path) {
                    return MethodReceiverHint::TypePath(ty);
                }
            }
            // Local name with a known one-hop type.
            if segs.len() == 1 {
                if let Some(types) = local_types {
                    if let Some(ty) = types.get(&segs[0]) {
                        return MethodReceiverHint::TypePath(ty.clone());
                    }
                }
            }
        }
    }

    // `self.field` — field's declared type on the impl's Self (resolved later).
    if let Some(field_expr) = ast::FieldExpr::cast(expr.syntax().clone()) {
        if let Some(name_ref) = field_expr.name_ref() {
            let field = name_ref.text().to_string();
            if is_ident(&field) {
                if let Some(base) = field_expr.expr() {
                    let mut base_expr = base;
                    for _ in 0..3 {
                        if let Some(ref_expr) = ast::RefExpr::cast(base_expr.syntax().clone()) {
                            if let Some(inner) = ref_expr.expr() {
                                base_expr = inner;
                                continue;
                            }
                        }
                        if let Some(paren) = ast::ParenExpr::cast(base_expr.syntax().clone()) {
                            if let Some(inner) = paren.expr() {
                                base_expr = inner;
                                continue;
                            }
                        }
                        break;
                    }
                    if let Some(path_expr) = ast::PathExpr::cast(base_expr.syntax().clone()) {
                        if let Some(path) = path_expr.path() {
                            let segs = path_segments(&path);
                            if segs.len() == 1 && segs[0] == "self" {
                                return MethodReceiverHint::SelfField(field);
                            }
                        }
                    }
                }
            }
        }
    }

    // `Type::assoc(...)` as the receiver expression.
    if let Some(call_expr) = ast::CallExpr::cast(expr.syntax().clone()) {
        if let Some(callee) = call_expr.expr() {
            if let Some(path_expr) = ast::PathExpr::cast(callee.syntax().clone()) {
                if let Some(path) = path_expr.path() {
                    let segs = path_segments(&path);
                    if segs.len() >= 2 {
                        let type_segs = &segs[..segs.len() - 1];
                        if type_segs
                            .iter()
                            .all(|s| is_ident(s) || matches!(s.as_str(), "crate" | "self" | "super"))
                        {
                            let abs = absolutize_local_type_path(type_segs, file_module_path);
                            return MethodReceiverHint::TypePath(abs);
                        }
                    }
                }
            }
        }
    }

    // `Type { … }` record constructor as receiver.
    if let Some(rec) = ast::RecordExpr::cast(expr.syntax().clone()) {
        if let Some(path) = rec.path() {
            let segs = path_segments(&path);
            if !segs.is_empty() {
                let abs = absolutize_local_type_path(&segs, file_module_path);
                return MethodReceiverHint::TypePath(abs);
            }
        }
    }

    MethodReceiverHint::Unknown
}

/// Map free-function names in this file to return-type paths with `&` /
/// `Option` / `Result` peeled. Used only for `let Some(x) = f()` /
/// `let Ok(x) = f()` bindings — the pattern itself unwraps the wrapper.
fn file_function_return_types(
    raw_defs: &[RawDef],
    file_module_path: &str,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for def in raw_defs {
        // Inherent methods use `Type::name` paths — skip; callers use bare names.
        if def.receiver_type_path.is_some() {
            continue;
        }
        let Some(func) = ast::Fn::cast(def.syntax.clone()) else {
            continue;
        };
        let Some(ret) = func.ret_type() else {
            continue;
        };
        let Some(ty) = ret.ty() else {
            continue;
        };
        if let Some(path) = peel_return_type_path(&ty, file_module_path) {
            out.insert(def.name.clone(), path);
        }
    }
    out
}

/// Map local binding names → absolutized type paths from annotations,
/// constructor RHS forms, or a local function's declared return type (one hop).
fn local_binding_types(
    fn_node: &SyntaxNode,
    file_module_path: &str,
    return_types: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();

    // Parameters with explicit types: `cache: Cache`, `cache: &Cache`.
    if let Some(func) = ast::Fn::cast(fn_node.clone()) {
        if let Some(params) = func.param_list() {
            for param in params.params() {
                let Some(pat) = param.pat() else {
                    continue;
                };
                let Some(name) = single_ident_pat(&pat) else {
                    continue;
                };
                if let Some(ty) = param.ty() {
                    if let Some(path) = simple_type_path(&ty, file_module_path) {
                        out.insert(name, path);
                    }
                }
            }
        }
    }

    // `let name: Type = …`, `let name = Type::…` / `Type { … }`, and
    // `let Some(name) = local_fn()` / `let Ok(name) = local_fn()`.
    for node in fn_node.descendants() {
        // Skip nested free-function / trait-impl bodies.
        if node.kind() == SyntaxKind::FN && &node != fn_node {
            continue;
        }
        let Some(let_stmt) = ast::LetStmt::cast(node) else {
            continue;
        };
        let Some(pat) = let_stmt.pat() else {
            continue;
        };
        if let Some(ty) = let_stmt.ty() {
            if let Some(name) = single_ident_pat(&pat) {
                if let Some(path) = simple_type_path(&ty, file_module_path) {
                    out.insert(name, path);
                    continue;
                }
            }
        }
        let Some(init) = let_stmt.initializer() else {
            continue;
        };
        if let Some(name) = single_ident_pat(&pat) {
            if let Some(path) = constructor_type_path(&init, file_module_path) {
                out.insert(name, path);
                continue;
            }
        }
        // `let Some(name) = local_fn()` / `let Ok(name) = …` — certain from the
        // callee's declared return type after peeling the matched wrapper.
        if let Some(name) = option_or_result_binding(&pat) {
            if let Some(path) = call_return_type_path(&init, return_types) {
                out.insert(name, path);
            }
        }
    }
    out
}

fn option_or_result_binding(pat: &ast::Pat) -> Option<String> {
    let tsp = ast::TupleStructPat::cast(pat.syntax().clone())?;
    let path = tsp.path()?;
    let segs = path_segments(&path);
    let last = segs.last()?.as_str();
    if !matches!(last, "Some" | "Ok") {
        return None;
    }
    let mut idents = tsp.fields().filter_map(|p| single_ident_pat(&p));
    let name = idents.next()?;
    if idents.next().is_some() {
        return None;
    }
    Some(name)
}

fn call_return_type_path(
    expr: &ast::Expr,
    return_types: &HashMap<String, String>,
) -> Option<String> {
    let call = ast::CallExpr::cast(expr.syntax().clone())?;
    let callee = call.expr()?;
    let path_expr = ast::PathExpr::cast(callee.syntax().clone())?;
    let path = path_expr.path()?;
    let segs = path_segments(&path);
    // Unqualified local call only — same honesty bar as other one-hop hints.
    if segs.len() != 1 {
        return None;
    }
    return_types.get(&segs[0]).cloned()
}

/// Peel `&` / `Option` / `Result` wrappers from a return type to a named path.
fn peel_return_type_path(ty: &ast::Type, file_module_path: &str) -> Option<String> {
    let mut cur = ty.clone();
    for _ in 0..6 {
        if let Some(ref_ty) = ast::RefType::cast(cur.syntax().clone()) {
            cur = ref_ty.ty()?;
            continue;
        }
        if let Some(paren) = ast::ParenType::cast(cur.syntax().clone()) {
            cur = paren.ty()?;
            continue;
        }
        let path_ty = ast::PathType::cast(cur.syntax().clone())?;
        let path = path_ty.path()?;
        let segs = path_segments(&path);
        if segs.is_empty() {
            return None;
        }
        if segs.len() == 1 && matches!(segs[0].as_str(), "Option" | "Result") {
            if let Some(inner) = first_generic_type_arg(&path) {
                cur = inner;
                continue;
            }
            return None;
        }
        if segs.len() == 1 && (is_primitive_type_name(&segs[0]) || is_prelude_type_name(&segs[0])) {
            return None;
        }
        return Some(absolutize_local_type_path(&segs, file_module_path));
    }
    None
}

fn first_generic_type_arg(path: &ast::Path) -> Option<ast::Type> {
    let seg = path.segment()?;
    let args = seg.generic_arg_list()?;
    for arg in args.generic_args() {
        if let ast::GenericArg::TypeArg(ta) = arg {
            return ta.ty();
        }
    }
    None
}

fn single_ident_pat(pat: &ast::Pat) -> Option<String> {
    let ident = ast::IdentPat::cast(pat.syntax().clone())?;
    let name = ident.name()?.text().to_string();
    if name == "_" {
        None
    } else {
        Some(name)
    }
}

fn simple_type_path(ty: &ast::Type, file_module_path: &str) -> Option<String> {
    // Peel references: `&Cache`, `&mut Cache`.
    let mut cur = ty.clone();
    for _ in 0..3 {
        if let Some(ref_ty) = ast::RefType::cast(cur.syntax().clone()) {
            cur = ref_ty.ty()?;
            continue;
        }
        if let Some(paren) = ast::ParenType::cast(cur.syntax().clone()) {
            cur = paren.ty()?;
            continue;
        }
        break;
    }
    let path_ty = ast::PathType::cast(cur.syntax().clone())?;
    let path = path_ty.path()?;
    let segs = path_segments(&path);
    if segs.is_empty() {
        return None;
    }
    if segs.len() == 1 && (is_primitive_type_name(&segs[0]) || is_prelude_type_name(&segs[0])) {
        return None;
    }
    Some(absolutize_local_type_path(&segs, file_module_path))
}

fn constructor_type_path(expr: &ast::Expr, file_module_path: &str) -> Option<String> {
    if let Some(call) = ast::CallExpr::cast(expr.syntax().clone()) {
        let callee = call.expr()?;
        let path_expr = ast::PathExpr::cast(callee.syntax().clone())?;
        let path = path_expr.path()?;
        let segs = path_segments(&path);
        if segs.len() >= 2 {
            let type_segs = &segs[..segs.len() - 1];
            return Some(absolutize_local_type_path(type_segs, file_module_path));
        }
        // Tuple struct constructor `Tag(1)` — single capitalised segment.
        if segs.len() == 1 && is_upper_camel_segment(&segs[0]) {
            return Some(absolutize_local_type_path(&segs, file_module_path));
        }
    }
    if let Some(rec) = ast::RecordExpr::cast(expr.syntax().clone()) {
        let path = rec.path()?;
        let segs = path_segments(&path);
        if !segs.is_empty() {
            return Some(absolutize_local_type_path(&segs, file_module_path));
        }
    }
    None
}

fn is_upper_camel_segment(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {
            chars.any(|c| c.is_ascii_lowercase()) && !name.contains('_')
        }
        _ => false,
    }
}

/// Remap [`FileFacts::method_receivers`] keys after crate-wide id assignment.
pub fn remap_method_receivers(
    receivers: &mut HashMap<FunctionId, String>,
    remap: &HashMap<FunctionId, FunctionId>,
) {
    let old = std::mem::take(receivers);
    for (id, path) in old {
        let new_id = remap.get(&id).cloned().unwrap_or(id);
        receivers.insert(new_id, path);
    }
}

fn is_path_like_callee(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    path.split("::").all(|seg| {
        let seg = seg.split('<').next().unwrap_or(seg);
        is_ident(seg) || matches!(seg, "crate" | "self" | "super")
    })
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    }
}

fn squish(s: &str) -> String {
    s.split_whitespace().collect()
}

struct LineIndex {
    /// Byte offset of the start of each 1-based line (`starts[0] == 0`).
    starts: Vec<u32>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push((i + 1) as u32);
            }
        }
        Self { starts }
    }

    fn line_of(&self, offset: u32) -> u32 {
        let idx = self.starts.partition_point(|&s| s <= offset);
        idx as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_engine::parse::parse_source;

    #[test]
    fn assigns_line_suffix_only_on_collision() {
        let source = r#"
fn unique() {}

#[cfg(unix)]
fn open() {}

#[cfg(windows)]
fn open() {}
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let mut facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        assign_function_ids("demo", &mut facts.functions);
        let ids: Vec<_> = facts.functions.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"demo::unique"));
        assert!(ids.iter().any(|id| id.starts_with("demo::open#L")));
        assert_eq!(
            ids.iter()
                .filter(|id| id.starts_with("demo::open#L"))
                .count(),
            2
        );
    }

    #[test]
    fn function_byte_range_spans_full_fn_node_including_attrs_and_docs() {
        // Slice assertions pin the settled extent: full `ast::Fn` syntax node,
        // not `fn_token()` (which would start at `fn` / `pub`) and not body-only.
        let source = "\
/// Docs above attribute.
#[inline]
pub fn gamma() {}

fn plain() {}

#[cfg(unix)]
fn open() {}

#[cfg(windows)]
fn open() {}
";
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        let by_name = |name: &str| {
            facts
                .functions
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };

        let gamma = by_name("gamma");
        let gamma_slice = &source[gamma.byte_start as usize..gamma.byte_end as usize];
        assert!(
            gamma_slice.starts_with("/// Docs above attribute."),
            "range must begin at the outer doc, not at fn/pub; got {:?}",
            gamma_slice.chars().take(40).collect::<String>()
        );
        assert!(
            gamma_slice.contains("#[inline]"),
            "range must include the intervening attribute"
        );
        assert!(
            gamma_slice.ends_with('}'),
            "range must end at the closing brace"
        );

        let plain = by_name("plain");
        let plain_slice = &source[plain.byte_start as usize..plain.byte_end as usize];
        assert_eq!(plain_slice, "fn plain() {}");

        let opens: Vec<_> = facts.functions.iter().filter(|f| f.name == "open").collect();
        assert_eq!(opens.len(), 2);
        for open in &opens {
            let slice = &source[open.byte_start as usize..open.byte_end as usize];
            assert!(
                slice.starts_with("#[cfg("),
                "cfg-duplicate range must begin at the attribute; got {:?}",
                slice.chars().take(40).collect::<String>()
            );
            assert!(slice.ends_with('}'));
        }
    }

    #[test]
    fn file_module_path_prefixes_definitions() {
        let source = r#"
pub fn get() {}
mod inner {
    pub fn helper() {}
}
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate::shapes", "2021").unwrap();
        let paths: Vec<_> = facts.functions.iter().map(|f| f.module_path.as_str()).collect();
        assert!(paths.contains(&"crate::shapes::get"));
        assert!(paths.contains(&"crate::shapes::inner::helper"));
    }

    #[test]
    fn module_level_const_call_is_file_owned() {
        let source = r#"
const fn compute_max() -> usize { 64 }
const MAX: usize = compute_max();
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        let file_calls: Vec<_> = facts
            .call_sites
            .iter()
            .filter(|c| c.owner == CallOwnerKind::File)
            .collect();
        assert_eq!(file_calls.len(), 1);
        assert_eq!(file_calls[0].call_path, "compute_max");
        assert!(file_calls[0].enclosing_function.is_none());
    }

    #[test]
    fn extracts_enums_structs_and_variants() {
        let source = r#"
pub enum Target {
    Resolved,
    Conflict,
}
pub struct Id(pub u32);
pub trait Marker {}
pub type Alias = Id;
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        assert_eq!(facts.types.len(), 4);
        let en = facts.types.iter().find(|t| t.name == "Target").unwrap();
        assert_eq!(en.kind, TypeKind::Enum);
        assert_eq!(en.variants, vec!["Resolved", "Conflict"]);
        assert_eq!(en.visibility, ItemVisibility::Public);
        assert!(facts.types.iter().any(|t| t.name == "Id" && t.kind == TypeKind::Struct));
        assert!(facts.types.iter().any(|t| t.name == "Marker" && t.kind == TypeKind::Trait));
        assert!(facts
            .types
            .iter()
            .any(|t| t.name == "Alias" && t.kind == TypeKind::TypeAlias));
    }

    #[test]
    fn flattens_nested_use_trees_and_self() {
        let source = r#"
use crate::{
    alpha::{one, two as second},
    beta::three,
};
use crate::alpha::{self, four};
use crate::beta::Marker as _;
use crate::shapes::*;
use std::fs;
pub use numbers::mean;
pub use text::upper as shout_upper;
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();

        let by_local = |name: &str| {
            facts
                .imports
                .iter()
                .find(|i| !i.is_glob && i.local_name() == Some(name))
                .unwrap_or_else(|| panic!("missing import {name}"))
        };

        assert_eq!(by_local("one").path, "crate::alpha::one");
        assert_eq!(by_local("second").path, "crate::alpha::two");
        assert_eq!(by_local("three").path, "crate::beta::three");
        assert_eq!(by_local("alpha").path, "crate::alpha");
        assert_eq!(by_local("four").path, "crate::alpha::four");
        // Bare paths stay bare until ResolveIndex::build normalizes them
        // against the module tree / extern crate set (edition 2018+ rules).
        assert_eq!(by_local("mean").path, "numbers::mean");
        assert!(by_local("mean").is_public);
        assert_eq!(by_local("shout_upper").path, "text::upper");
        assert_eq!(by_local("fs").path, "std::fs");

        assert!(facts.imports.iter().any(|i| i.is_glob && i.path == "crate::shapes"));
        assert!(
            !facts.imports.iter().any(|i| i.path.contains("Marker")),
            "`as _` must bind nothing"
        );
    }

    #[test]
    fn function_body_use_is_attributed_to_enclosing_module() {
        // Known limitation: body-scoped `use` is recorded as module-wide.
        let source = r#"
fn f() {
    use crate::helper;
    helper();
}
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        let imp = facts
            .imports
            .iter()
            .find(|i| i.local_name() == Some("helper"))
            .expect("helper import");
        assert_eq!(imp.module_path, "crate");
    }

    #[test]
    fn extracts_local_bindings_but_not_nested_fn_names() {
        let source = r#"
fn run(cb: fn()) {
    let by_name = || {};
    fn nested() {}
    by_name();
    nested();
    cb();
}
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        let run = facts.functions.iter().find(|f| f.name == "run").unwrap();
        let locals = facts.local_bindings.get(&run.id).expect("run locals");
        assert!(locals.contains("by_name"), "{locals:?}");
        assert!(locals.contains("cb"), "{locals:?}");
        assert!(
            !locals.contains("nested"),
            "nested fn name is a definition, not a local binding: {locals:?}"
        );
        let nested = facts.functions.iter().find(|f| f.name == "nested").unwrap();
        assert!(
            nested.module_path.ends_with("run::nested"),
            "{}",
            nested.module_path
        );
    }

    #[test]
    fn inline_mod_use_super_is_attributed_to_child_module() {
        let source = r#"
fn helper() {}
mod tests {
    use super::*;
    use super::helper;
    fn t() { helper(); }
}
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        assert!(
            facts
                .imports
                .iter()
                .any(|i| i.is_glob && i.module_path == "crate::tests" && i.path == "crate"),
            "use super::* must land on crate::tests → crate: {:?}",
            facts.imports
        );
        let explicit = facts
            .imports
            .iter()
            .find(|i| !i.is_glob && i.local_name() == Some("helper"))
            .expect("explicit use super::helper");
        assert_eq!(explicit.module_path, "crate::tests");
        assert_eq!(explicit.path, "crate::helper");
    }

    #[test]
    fn extracts_doc_comments_with_pinned_whitespace() {
        use crate::map::DocCommentKind;

        let source = r#"//! File-level module docs.
//! Second inner line.

/// Single-line outer.
pub fn alpha() {}

/// First outer line.
/// Second outer line.
pub fn beta() {}

/// Docs above attribute.
#[inline]
pub fn gamma() {}

/**Block outer docs.*/
pub fn delta() {}

// Ordinary comment — must not be collected.
pub fn epsilon() {}

#[doc = "Attr docs."]
pub fn zeta() {}
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();

        assert_eq!(facts.doc_comments.len(), 1);
        assert_eq!(facts.doc_comments[0].kind, DocCommentKind::Inner);
        assert_eq!(
            facts.doc_comments[0].text,
            "File-level module docs.\nSecond inner line."
        );

        let by_name = |name: &str| {
            facts
                .functions
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };

        assert_eq!(by_name("alpha").doc_comments.len(), 1);
        assert_eq!(by_name("alpha").doc_comments[0].kind, DocCommentKind::Outer);
        assert_eq!(by_name("alpha").doc_comments[0].text, "Single-line outer.");

        assert_eq!(
            by_name("beta").doc_comments[0].text,
            "First outer line.\nSecond outer line."
        );
        assert_eq!(
            by_name("gamma").doc_comments[0].text,
            "Docs above attribute."
        );
        assert_eq!(by_name("delta").doc_comments[0].text, "Block outer docs.");
        assert!(
            by_name("epsilon").doc_comments.is_empty(),
            "ordinary // must not become a DocComment"
        );
        assert_eq!(by_name("zeta").doc_comments[0].text, "Attr docs.");
    }

    #[test]
    fn recovers_calls_inside_allowlisted_macros_only() {
        let source = r#"
fn mean(v: i32) -> i32 { v }
fn helper() -> i32 { 1 }
struct Foo(i32);
fn run() {
    let _ = format!("{}", mean(1));
    let _ = matches!(Some(1), Some(_y));
    let _ = stringify!(helper());
    let _ = vec![Foo(1), helper()];
    let _ = println!("{}", format!("{}", helper()));
}
macro_rules! m {
    ($name:ident($a:expr)) => { $name($a) };
}
fn through_macro() { m!(mean(1)); }
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        let run_id = facts
            .functions
            .iter()
            .find(|f| f.name == "run")
            .unwrap()
            .id
            .clone();
        let in_run: Vec<_> = facts
            .call_sites
            .iter()
            .filter(|c| c.enclosing_function.as_ref() == Some(&run_id))
            .collect();
        let paths: Vec<&str> = in_run.iter().map(|c| c.call_path.as_str()).collect();
        assert!(paths.contains(&"mean"), "format! should recover mean: {paths:?}");
        assert!(
            paths.iter().filter(|p| **p == "helper").count() >= 2,
            "vec! + nested format! should recover helper: {paths:?}"
        );
        assert!(
            in_run
                .iter()
                .filter(|c| c.call_path == "mean" || c.call_path == "helper")
                .all(|c| c.from_macro),
            "recovered mean/helper sites must set from_macro"
        );
        assert!(
            !paths.contains(&"Some"),
            "matches! must not recover pattern constructors: {paths:?}"
        );
        // `Foo(1)` may appear as a pending CallExpr; resolve drops constructors.

        let through = facts
            .functions
            .iter()
            .find(|f| f.name == "through_macro")
            .unwrap()
            .id
            .clone();
        assert!(
            !facts
                .call_sites
                .iter()
                .any(|c| c.enclosing_function.as_ref() == Some(&through)),
            "user macros / macro_rules bodies must stay closed"
        );
    }
}
