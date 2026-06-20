//! Language-specific, heuristic source analysis: symbol extraction, imports,
//! call detection and lightweight syntax highlighting.

use std::collections::HashSet;
use std::path::Path;

use super::{FileInfo, FuncSym, TypeSym};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    C,
    Cpp,
}

impl Lang {
    pub fn from_path(path: &Path) -> Option<Lang> {
        Lang::from_ext(&path.to_string_lossy())
    }

    pub fn from_ext(path: &str) -> Option<Lang> {
        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
        Some(match ext.as_str() {
            "rs" => Lang::Rust,
            "py" | "pyi" => Lang::Python,
            "js" | "jsx" | "mjs" | "cjs" => Lang::JavaScript,
            "ts" | "tsx" | "mts" | "cts" => Lang::TypeScript,
            "go" => Lang::Go,
            "java" => Lang::Java,
            "c" | "h" => Lang::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Lang::Cpp,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Rust => "Rust",
            Lang::Python => "Python",
            Lang::JavaScript => "JavaScript",
            Lang::TypeScript => "TypeScript",
            Lang::Go => "Go",
            Lang::Java => "Java",
            Lang::C => "C",
            Lang::Cpp => "C++",
        }
    }

    fn line_comment(self) -> &'static str {
        match self {
            Lang::Python => "#",
            _ => "//",
        }
    }

    fn brace_based(self) -> bool {
        !matches!(self, Lang::Python)
    }

    pub fn has_io(self, text: &str) -> bool {
        const PATTERNS: &[&str] = &[
            "fs::", "File::", "std::io", "println!", "print!", "eprintln!",
            "read_to_string", "fs.read", "fs.write", "fs.promises", "open(",
            "fopen", "fetch(", "requests.", "urllib", "http.", "https.",
            "fmt.Print", "os.Open", "System.out", "Scanner(", "FileReader",
            "socket", "TcpListener", "axios",
        ];
        PATTERNS.iter().any(|p| text.contains(p))
    }

    /// Top-of-file documentation comment, if any.
    pub fn file_doc(self, text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        // skip shebang / blank
        while i < lines.len() && (lines[i].trim().is_empty() || lines[i].starts_with("#!")) {
            i += 1;
        }
        if self == Lang::Python {
            // module docstring
            let t = lines.get(i).map(|s| s.trim()).unwrap_or("");
            if t.starts_with("\"\"\"") || t.starts_with("'''") {
                let q = &t[..3];
                let rest = &t[3..];
                if let Some(end) = rest.find(q) {
                    return rest[..end].trim().to_string();
                }
                let mut buf = vec![rest.to_string()];
                let mut j = i + 1;
                while j < lines.len() {
                    if let Some(end) = lines[j].find(q) {
                        buf.push(lines[j][..end].to_string());
                        break;
                    }
                    buf.push(lines[j].to_string());
                    j += 1;
                }
                return buf.join(" ").trim().to_string();
            }
            // leading # comments
            let mut docs = Vec::new();
            while i < lines.len() && lines[i].trim_start().starts_with('#') {
                docs.push(lines[i].trim_start().trim_start_matches('#').trim().to_string());
                i += 1;
            }
            return docs.join(" ").trim().to_string();
        }
        // C-like: leading // or //! or /* */ comment block
        let mut docs = Vec::new();
        while i < lines.len() {
            let t = lines[i].trim_start();
            if t.starts_with("//") {
                docs.push(t.trim_start_matches('/').trim_start_matches('!').trim().to_string());
                i += 1;
            } else if t.starts_with("/*") {
                let mut seg = t.trim_start_matches("/*").to_string();
                if let Some(end) = seg.find("*/") {
                    docs.push(seg[..end].trim().to_string());
                    break;
                }
                docs.push(seg.trim().to_string());
                i += 1;
                while i < lines.len() {
                    seg = lines[i].trim().to_string();
                    if let Some(end) = seg.find("*/") {
                        docs.push(seg[..end].trim_matches('*').trim().to_string());
                        break;
                    }
                    docs.push(seg.trim_start_matches('*').trim().to_string());
                    i += 1;
                }
                break;
            } else {
                break;
            }
        }
        docs.into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string()
    }

    /// Module/import hints used to infer file-level dependencies.
    pub fn imports(self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for raw in text.lines() {
            let t = raw.trim();
            match self {
                Lang::Rust => {
                    if let Some(rest) = t.strip_prefix("use ").or_else(|| t.strip_prefix("pub use ")) {
                        out.push(rest.trim_end_matches(';').to_string());
                    } else if let Some(rest) = t.strip_prefix("mod ") {
                        out.push(rest.trim_end_matches(';').trim().to_string());
                    }
                }
                Lang::Python => {
                    if let Some(rest) = t.strip_prefix("from ") {
                        if let Some((module, _)) = rest.split_once(" import") {
                            out.push(module.trim().to_string());
                        }
                    } else if let Some(rest) = t.strip_prefix("import ") {
                        for part in rest.split(',') {
                            let m = part.trim().split_whitespace().next().unwrap_or("");
                            out.push(m.to_string());
                        }
                    }
                }
                Lang::JavaScript | Lang::TypeScript => {
                    if t.starts_with("import") || t.contains("require(") || t.contains("from ") {
                        if let Some(p) = extract_quoted(t) {
                            out.push(p);
                        }
                    }
                }
                Lang::Go => {
                    if let Some(p) = extract_quoted(t) {
                        if t.starts_with('"') || t.contains("import") || t.starts_with('_') {
                            out.push(p);
                        }
                    }
                }
                Lang::Java => {
                    if let Some(rest) = t.strip_prefix("import ") {
                        out.push(rest.trim_end_matches(';').replace("static ", ""));
                    }
                }
                Lang::C | Lang::Cpp => {
                    if t.starts_with("#include") {
                        if let Some(p) = extract_quoted(t) {
                            out.push(p);
                        }
                    }
                }
            }
        }
        out
    }

    /// Extract function and type symbols from a file.
    pub fn extract(self, text: &str) -> (Vec<FuncSym>, Vec<TypeSym>) {
        let lines: Vec<&str> = text.lines().collect();
        let mut funcs: Vec<FuncSym> = Vec::new();
        let mut types: Vec<TypeSym> = Vec::new();
        let mut fn_ids: HashSet<String> = HashSet::new();
        let mut ty_ids: HashSet<String> = HashSet::new();

        for (idx, line) in lines.iter().enumerate() {
            if let Some((name, sig)) = self.decl_func(line) {
                if is_keyword(&name) {
                    continue;
                }
                let doc = gather_doc(&lines, idx, self);
                let body = self.body_of(&lines, idx);
                let is_test = self.is_test_fn(&lines, idx, &name);
                let is_pub = line.contains("pub ")
                    || line.contains("export ")
                    || line.trim_start().starts_with("public ")
                    || self == Lang::Python && !name.starts_with('_')
                    || self == Lang::Go && name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
                let id = unique(&name, &mut fn_ids);
                funcs.push(FuncSym {
                    id,
                    name,
                    line: idx + 1,
                    sig,
                    doc,
                    body,
                    is_test,
                    is_pub,
                });
                continue;
            }
            if let Some((name, sig)) = self.decl_type(line) {
                if is_keyword(&name) {
                    continue;
                }
                let doc = gather_doc(&lines, idx, self);
                let body = self.body_of(&lines, idx);
                let id = unique(&name, &mut ty_ids);
                types.push(TypeSym {
                    id,
                    name,
                    line: idx + 1,
                    sig,
                    doc,
                    body,
                });
            }
        }
        (funcs, types)
    }

    fn decl_func(self, line: &str) -> Option<(String, String)> {
        let t = line.trim_start();
        match self {
            Lang::Rust => {
                let inner = strip_prefixes(t, &["pub(crate) ", "pub ", "async ", "const ", "unsafe ", "extern ", "default "]);
                let inner = strip_prefixes(inner, &["async ", "unsafe ", "extern ", "extern \"C\" "]);
                let rest = inner.strip_prefix("fn ")?;
                let name = first_ident(rest);
                if name.is_empty() {
                    return None;
                }
                let after = &rest[name.len()..];
                Some((name, clean_sig(after)))
            }
            Lang::Python => {
                let inner = t.strip_prefix("async ").unwrap_or(t);
                let rest = inner.strip_prefix("def ")?;
                let name = first_ident(rest);
                if name.is_empty() {
                    return None;
                }
                let after = &rest[name.len()..];
                Some((name, clean_sig(after)))
            }
            Lang::JavaScript | Lang::TypeScript => {
                let inner = strip_prefixes(t, &["export default ", "export ", "async ", "static ", "public ", "private ", "protected "]);
                let inner = strip_prefixes(inner, &["async ", "function* ", "function "]);
                if let Some(name) = inner.strip_prefix("").map(first_ident) {
                    // function declaration: original had "function " stripped
                    if t.contains("function") {
                        if !name.is_empty() {
                            let after = &inner[name.len()..];
                            return Some((name, clean_sig(after)));
                        }
                    }
                }
                // const f = (..) => / arrow
                for kw in &["const ", "let ", "var "] {
                    if let Some(after_kw) = inner.strip_prefix(kw) {
                        let name = first_ident(after_kw);
                        if name.is_empty() {
                            continue;
                        }
                        let after = after_kw[name.len()..].trim_start();
                        if let Some(rhs) = after.strip_prefix('=') {
                            let rhs = rhs.trim_start();
                            let rhs2 = strip_prefixes(rhs, &["async "]);
                            if rhs2.starts_with('(') || rhs2.starts_with("function") {
                                return Some((name, "(…)".into()));
                            }
                        }
                    }
                }
                None
            }
            Lang::Go => {
                let rest = t.strip_prefix("func ")?;
                let rest = if rest.starts_with('(') {
                    rest.split_once(')').map(|(_, a)| a.trim_start()).unwrap_or(rest)
                } else {
                    rest
                };
                let name = first_ident(rest);
                if name.is_empty() {
                    return None;
                }
                let after = &rest[name.len()..];
                Some((name, clean_sig(after)))
            }
            Lang::Java => {
                if !t.contains('(') || t.trim_end().ends_with(';') {
                    return None;
                }
                let before = t.split('(').next()?;
                if before.contains('=') {
                    return None;
                }
                let words: Vec<&str> = before.split_whitespace().collect();
                let cand = words.last().copied().unwrap_or("");
                let cand = cand.split('.').last().unwrap_or("");
                if cand.is_empty() || words.len() < 2 || is_keyword(cand) {
                    return None;
                }
                if cand.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    let after = &t[t.find('(').unwrap()..];
                    Some((cand.to_string(), clean_sig(after)))
                } else {
                    None
                }
            }
            Lang::C | Lang::Cpp => {
                let te = t.trim_end();
                if !t.contains('(') || te.ends_with(';') || te.ends_with(')') {
                    return None;
                }
                if t.contains('=') {
                    return None;
                }
                let before = t.split('(').next()?;
                let cand: String = before
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .filter(|s| !s.is_empty())
                    .last()
                    .unwrap_or("")
                    .to_string();
                if cand.is_empty() || is_keyword(&cand) {
                    return None;
                }
                if cand.starts_with(|c: char| c.is_alphabetic() || c == '_') {
                    let after = &t[t.find('(').unwrap()..];
                    Some((cand, clean_sig(after)))
                } else {
                    None
                }
            }
        }
    }

    fn decl_type(self, line: &str) -> Option<(String, String)> {
        let t = line.trim_start();
        match self {
            Lang::Rust => {
                let inner = strip_prefixes(t, &["pub(crate) ", "pub ", "default "]);
                for kw in &["struct ", "enum ", "trait ", "union ", "type "] {
                    if let Some(rest) = inner.strip_prefix(kw) {
                        let name = first_ident(rest);
                        if !name.is_empty() {
                            return Some((name, kw.trim().to_string()));
                        }
                    }
                }
                None
            }
            Lang::Python => t
                .strip_prefix("class ")
                .map(first_ident)
                .filter(|s| !s.is_empty())
                .map(|n| (n, "class".into())),
            Lang::JavaScript | Lang::TypeScript => {
                let inner = strip_prefixes(t, &["export default ", "export ", "abstract "]);
                for kw in &["class ", "interface ", "type ", "enum "] {
                    if let Some(rest) = inner.strip_prefix(kw) {
                        let name = first_ident(rest);
                        if !name.is_empty() {
                            return Some((name, kw.trim().to_string()));
                        }
                    }
                }
                None
            }
            Lang::Go => {
                let rest = t.strip_prefix("type ")?;
                let name = first_ident(rest);
                if name.is_empty() {
                    return None;
                }
                let after = rest[name.len()..].trim_start();
                if after.starts_with("struct") {
                    Some((name, "struct".into()))
                } else if after.starts_with("interface") {
                    Some((name, "interface".into()))
                } else {
                    Some((name, "type".into()))
                }
            }
            Lang::Java => {
                let inner = strip_prefixes(
                    t,
                    &["public ", "private ", "protected ", "abstract ", "final ", "static "],
                );
                let inner = strip_prefixes(inner, &["abstract ", "final ", "static "]);
                for kw in &["class ", "interface ", "enum ", "record "] {
                    if let Some(rest) = inner.strip_prefix(kw) {
                        let name = first_ident(rest);
                        if !name.is_empty() {
                            return Some((name, kw.trim().to_string()));
                        }
                    }
                }
                None
            }
            Lang::C | Lang::Cpp => {
                let inner = t.strip_prefix("typedef ").unwrap_or(t);
                for kw in &["struct ", "class ", "enum ", "union "] {
                    if let Some(rest) = inner.strip_prefix(kw) {
                        let name = first_ident(rest);
                        if !name.is_empty() {
                            return Some((name, kw.trim().to_string()));
                        }
                    }
                }
                None
            }
        }
    }

    /// Body text of a declaration starting on `lines[idx]`.
    fn body_of(self, lines: &[&str], idx: usize) -> String {
        if self.brace_based() {
            // join from idx, find first '{', collect to matching '}'
            let mut buf = String::new();
            let mut depth: i32 = 0;
            let mut started = false;
            let mut count = 0;
            for line in &lines[idx..] {
                count += 1;
                for ch in line.chars() {
                    if ch == '{' {
                        depth += 1;
                        started = true;
                    } else if ch == '}' {
                        depth -= 1;
                    }
                    if started {
                        buf.push(ch);
                    }
                }
                buf.push('\n');
                if started && depth <= 0 {
                    break;
                }
                if count > 400 {
                    break;
                }
            }
            buf
        } else {
            // python indentation
            let base = indent_of(lines[idx]);
            let mut buf = String::new();
            for line in &lines[idx + 1..] {
                if line.trim().is_empty() {
                    buf.push('\n');
                    continue;
                }
                if indent_of(line) <= base {
                    break;
                }
                buf.push_str(line);
                buf.push('\n');
            }
            buf
        }
    }

    fn is_test_fn(self, lines: &[&str], idx: usize, name: &str) -> bool {
        match self {
            Lang::Rust => {
                let mut i = idx;
                while i > 0 {
                    i -= 1;
                    let t = lines[i].trim();
                    if t.is_empty() {
                        continue;
                    }
                    if t.starts_with("#[") {
                        if t.contains("test") {
                            return true;
                        }
                        continue;
                    }
                    break;
                }
                false
            }
            Lang::Python => name.starts_with("test_") || name == "test",
            Lang::Go => name.starts_with("Test") && name.len() > 4,
            Lang::Java => {
                let mut i = idx;
                while i > 0 {
                    i -= 1;
                    let t = lines[i].trim();
                    if t.is_empty() {
                        continue;
                    }
                    if t.starts_with('@') {
                        if t.contains("Test") {
                            return true;
                        }
                        continue;
                    }
                    break;
                }
                name.starts_with("test")
            }
            _ => name.starts_with("test") || name.starts_with("Test"),
        }
    }

    /// Identifiers that appear as a call `name(` inside `body`.
    pub fn calls_in(self, body: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let chars: Vec<char> = body.chars().collect();
        let n = chars.len();
        let mut i = 0;
        while i < n {
            let c = chars[i];
            if c.is_alphabetic() || c == '_' {
                let start = i;
                while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                // skip whitespace
                let mut j = i;
                while j < n && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < n && chars[j] == '(' && !is_keyword(&ident) {
                    if seen.insert(ident.clone()) {
                        out.push(ident);
                    }
                }
            } else {
                i += 1;
            }
        }
        out
    }

    fn keywords(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
                "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static",
                "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
            Lang::Python => &[
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
                "del", "elif", "else", "except", "finally", "for", "from", "global", "if",
                "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise",
                "return", "try", "while", "with", "yield", "True", "False", "None", "self",
            ],
            Lang::JavaScript | Lang::TypeScript => &[
                "async", "await", "break", "case", "catch", "class", "const", "continue",
                "default", "delete", "do", "else", "enum", "export", "extends", "false",
                "finally", "for", "function", "if", "implements", "import", "in", "instanceof",
                "interface", "let", "new", "null", "of", "return", "super", "switch", "this",
                "throw", "true", "try", "type", "typeof", "var", "void", "while", "yield",
            ],
            Lang::Go => &[
                "break", "case", "chan", "const", "continue", "default", "defer", "else",
                "fallthrough", "for", "func", "go", "goto", "if", "import", "interface", "map",
                "package", "range", "return", "select", "struct", "switch", "type", "var",
                "nil", "true", "false",
            ],
            Lang::Java => &[
                "abstract", "boolean", "break", "case", "catch", "class", "const", "continue",
                "default", "do", "else", "enum", "extends", "final", "finally", "for", "if",
                "implements", "import", "instanceof", "interface", "new", "null", "package",
                "private", "protected", "public", "return", "static", "super", "switch", "this",
                "throw", "throws", "try", "void", "while", "true", "false", "record",
            ],
            Lang::C | Lang::Cpp => &[
                "auto", "break", "case", "char", "class", "const", "continue", "default",
                "delete", "do", "double", "else", "enum", "extern", "false", "float", "for",
                "goto", "if", "inline", "int", "long", "namespace", "new", "nullptr", "private",
                "protected", "public", "return", "short", "signed", "sizeof", "static", "struct",
                "switch", "template", "this", "true", "typedef", "typename", "union", "unsigned",
                "using", "virtual", "void", "volatile", "while",
            ],
        }
    }

    fn primitives(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &[
                "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128",
                "isize", "f32", "f64", "bool", "char", "str",
            ],
            Lang::Go => &["int", "int32", "int64", "uint", "float64", "float32", "bool", "string", "byte", "rune", "error"],
            Lang::Java | Lang::C | Lang::Cpp => {
                &["int", "long", "short", "float", "double", "bool", "char", "void", "boolean", "byte"]
            }
            _ => &[],
        }
    }

    /// A representative source snippet (for a whole file), highlighted.
    pub fn snippet(self, info: &FileInfo) -> Vec<(String, String)> {
        if info.text.lines().next().is_none() {
            return vec![("// empty file\n".into(), "c".into())];
        }
        let start = if let Some(f) = info
            .funcs
            .iter()
            .find(|f| f.name == "main")
            .or_else(|| info.funcs.iter().find(|f| f.is_pub))
            .or_else(|| info.funcs.first())
        {
            f.line
        } else if let Some(t) = info.types.first() {
            t.line
        } else {
            1
        };
        self.snippet_at(&info.text, start)
    }

    /// A highlighted source snippet starting at `start_line` (1-based) — used for
    /// a single function or data structure.
    pub fn snippet_at(self, text: &str, start_line: usize) -> Vec<(String, String)> {
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() || start_line == 0 {
            return vec![("// no source\n".into(), "c".into())];
        }
        let start = (start_line - 1).min(lines.len() - 1);
        let mut snippet_lines: Vec<&str> = Vec::new();
        if self.brace_based() {
            let mut depth: i32 = 0;
            let mut started = false;
            for line in &lines[start..] {
                snippet_lines.push(line);
                for ch in line.chars() {
                    if ch == '{' {
                        depth += 1;
                        started = true;
                    } else if ch == '}' {
                        depth -= 1;
                    }
                }
                if started && depth <= 0 {
                    break;
                }
                if snippet_lines.len() >= 18 {
                    break;
                }
            }
        } else {
            let base = indent_of(lines[start]);
            snippet_lines.push(lines[start]);
            for line in &lines[start + 1..] {
                if !line.trim().is_empty() && indent_of(line) <= base {
                    break;
                }
                snippet_lines.push(line);
                if snippet_lines.len() >= 18 {
                    break;
                }
            }
        }
        while snippet_lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            snippet_lines.pop();
        }
        highlight(&snippet_lines.join("\n"), self)
    }
}

// ── Module-level helpers ───────────────────────────────────────────────────

/// All distinct identifiers appearing in `text`.
pub fn idents(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            if seen.insert(ident.clone()) {
                out.push(ident);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// PascalCase identifiers referenced inside a type body (field types).
pub fn type_refs(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let chars: Vec<char> = body.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            let first = ident.chars().next().unwrap();
            if first.is_uppercase() && ident.len() >= 2 && !is_keyword(&ident) {
                if seen.insert(ident.clone()) {
                    out.push(ident);
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

const COMMON_WORDS: &[&str] = &[
    "string", "result", "option", "vec", "self", "none", "some", "true", "false", "error",
    "value", "data", "node", "item", "name", "path", "type", "kind", "list", "map", "set",
    "config", "args", "main", "run", "build", "parse", "write", "read", "init", "test",
    "new", "from", "into", "default", "clone", "debug", "hash", "json", "html", "this",
    "void", "null", "print", "object", "array", "number", "boolean", "func", "class",
    "import", "export", "return", "const", "char", "bool", "byte", "float", "double", "long",
    "with", "text", "line", "file", "size", "count", "index", "start", "stop", "next", "prev",
];

pub fn is_common_word(name: &str) -> bool {
    COMMON_WORDS.contains(&name.to_lowercase().as_str())
}

fn highlight(src: &str, lang: Lang) -> Vec<(String, String)> {
    let kws: HashSet<&str> = lang.keywords().iter().copied().collect();
    let prims: HashSet<&str> = lang.primitives().iter().copied().collect();
    let line_comment = lang.line_comment();
    let lc0 = line_comment.chars().next().unwrap();
    let lc_double = line_comment.len() == 2;
    let block = !matches!(lang, Lang::Python);

    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |s: String, k: &str| {
        if let Some(last) = out.last_mut() {
            if last.1 == k {
                last.0.push_str(&s);
                return;
            }
        }
        out.push((s, k.to_string()));
    };

    let mut i = 0;
    while i < n {
        let c = chars[i];
        // block comment
        if block && c == '/' && i + 1 < n && chars[i + 1] == '*' {
            let start = i;
            i += 2;
            while i < n && !(chars[i] == '*' && i + 1 < n && chars[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(n);
            push(chars[start..i].iter().collect(), "c");
            continue;
        }
        // line comment
        let is_line_comment = if lc_double {
            c == lc0 && i + 1 < n && chars[i + 1] == lc0
        } else {
            c == lc0
        };
        if is_line_comment {
            let start = i;
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            push(chars[start..i].iter().collect(), "c");
            continue;
        }
        // string / char literal
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            let start = i;
            i += 1;
            while i < n {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            push(chars[start..i.min(n)].iter().collect(), "");
            continue;
        }
        // identifier
        if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            let mut j = i;
            while j < n && (chars[j] == ' ' || chars[j] == '\t') {
                j += 1;
            }
            let kind = if kws.contains(ident.as_str()) {
                "kw"
            } else if prims.contains(ident.as_str()) {
                "ty"
            } else if j < n && chars[j] == '(' {
                "fn"
            } else if ident.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                "ty"
            } else {
                ""
            };
            push(ident, kind);
            continue;
        }
        // default
        push(c.to_string(), "");
        i += 1;
    }
    if out.is_empty() {
        out.push(("\n".into(), "".into()));
    }
    out
}

// ── small string helpers ───────────────────────────────────────────────────

fn extract_quoted(s: &str) -> Option<String> {
    let bytes: Vec<char> = s.chars().collect();
    for (i, &c) in bytes.iter().enumerate() {
        if c == '"' || c == '\'' {
            let rest: String = bytes[i + 1..].iter().collect();
            if let Some(end) = rest.find(c) {
                return Some(rest[..end].to_string());
            }
        }
        if c == '<' && s.contains("#include") {
            let rest: String = bytes[i + 1..].iter().collect();
            if let Some(end) = rest.find('>') {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

fn strip_prefixes<'a>(mut s: &'a str, prefixes: &[&str]) -> &'a str {
    let mut changed = true;
    while changed {
        changed = false;
        for p in prefixes {
            if let Some(rest) = s.strip_prefix(p) {
                s = rest.trim_start();
                changed = true;
            }
        }
    }
    s
}

fn first_ident(s: &str) -> String {
    s.trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect()
}

fn clean_sig(after: &str) -> String {
    let mut s = after.trim().to_string();
    for cut in ['{', ';'] {
        if let Some(pos) = s.find(cut) {
            s.truncate(pos);
        }
    }
    let s = s.trim().trim_end_matches(':').trim();
    let s = s.replace('\t', " ");
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > 90 {
        format!("{}…", &collapsed[..88.min(collapsed.len())])
    } else {
        collapsed
    }
}

fn gather_doc(lines: &[&str], decl_idx: usize, lang: Lang) -> String {
    let prefixes: &[&str] = match lang {
        Lang::Python => &["#"],
        _ => &["///", "//!", "//", "*", "/**", "/*"],
    };
    let mut docs = Vec::new();
    let mut i = decl_idx;
    while i > 0 {
        i -= 1;
        let t = lines[i].trim();
        if t.is_empty() {
            break;
        }
        if t.starts_with("#[") || t.starts_with('@') {
            // attribute/annotation, skip but keep scanning
            continue;
        }
        let mut matched = false;
        for p in prefixes {
            if t.starts_with(p) {
                let cleaned = t
                    .trim_start_matches('/')
                    .trim_start_matches('#')
                    .trim_start_matches('*')
                    .trim_start_matches('!')
                    .trim_end_matches("*/")
                    .trim();
                if !cleaned.is_empty() {
                    docs.push(cleaned.to_string());
                }
                matched = true;
                break;
            }
        }
        if !matched {
            break;
        }
    }
    docs.reverse();
    docs.join(" ").trim().to_string()
}

fn indent_of(line: &str) -> usize {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

fn unique(name: &str, used: &mut HashSet<String>) -> String {
    if used.insert(name.to_string()) {
        return name.to_string();
    }
    let mut n = 2;
    loop {
        let cand = format!("{}_{}", name, n);
        if used.insert(cand.clone()) {
            return cand;
        }
        n += 1;
    }
}

fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "if" | "for"
            | "while"
            | "switch"
            | "return"
            | "new"
            | "match"
            | "catch"
            | "else"
            | "try"
            | "do"
            | "case"
            | "fn"
            | "def"
            | "func"
            | "class"
            | "struct"
            | "enum"
            | "impl"
            | "where"
            | "async"
            | "await"
            | "pub"
            | "let"
            | "const"
            | "var"
            | "type"
            | "interface"
            | "trait"
            | "union"
            | "in"
            | "of"
            | "with"
            | "as"
            | "use"
            | "mod"
            | "self"
            | "super"
            | "sizeof"
    )
}
