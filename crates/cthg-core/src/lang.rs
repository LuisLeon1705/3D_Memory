//! Multi-language definitions and scanner dispatch for the project index.
//!
//! Provides common representations for symbols, imports, and calls across
//! Rust, Python, JavaScript/TypeScript, and HTML.

use std::collections::{HashMap, HashSet};

/// Supported language types in the code index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    Python,
    Js,
    Ts,
    Html,
}

/// Language family for cross-language call resolution and scoping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LangFamily {
    Rust,
    Python,
    Web,
}

impl Lang {
    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::Js => "js",
            Lang::Ts => "ts",
            Lang::Html => "html",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    pub fn family(&self) -> LangFamily {
        match self {
            Lang::Rust => LangFamily::Rust,
            Lang::Python => LangFamily::Python,
            Lang::Js | Lang::Ts | Lang::Html => LangFamily::Web,
        }
    }
}

/// Identifies the language of a file from its path extension.
pub fn lang_of(path: &str) -> Option<Lang> {
    let p = path.to_ascii_lowercase();
    if p.ends_with(".rs") {
        Some(Lang::Rust)
    } else if p.ends_with(".py") {
        Some(Lang::Python)
    } else if p.ends_with(".ts") || p.ends_with(".tsx") || p.ends_with(".mts") || p.ends_with(".cts") {
        Some(Lang::Ts)
    } else if p.ends_with(".js") || p.ends_with(".jsx") || p.ends_with(".mjs") || p.ends_with(".cjs") {
        Some(Lang::Js)
    } else if p.ends_with(".html") || p.ends_with(".htm") {
        Some(Lang::Html)
    } else {
        None
    }
}

/// A declared code symbol.
impl std::str::FromStr for Lang {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "rust" => Ok(Lang::Rust),
            "python" => Ok(Lang::Python),
            "js" => Ok(Lang::Js),
            "ts" => Ok(Lang::Ts),
            "html" => Ok(Lang::Html),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: usize,
    pub end_line: usize,
    pub public: bool,
    pub parent: Option<String>,
    pub test: bool,
    /// Function/method parameter and return type signature (or class bases / type definition).
    pub signature: String,
    /// Decorators associated with the symbol (e.g. `@app.route(...)`, `@staticmethod`).
    pub decorators: Vec<String>,
}

/// An import statement from a source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub raw: String,
    pub resolved: Option<String>,
}

/// A function call site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub from: String,
    pub name: String,
    pub line: usize,
    pub has_receiver: bool,
    pub qualifier: Option<String>,
}

impl Call {
    pub fn new(from: impl Into<String>, name: impl Into<String>, line: usize) -> Self {
        Self {
            from: from.into(),
            name: name.into(),
            line,
            has_receiver: false,
            qualifier: None,
        }
    }
}

/// Standard library methods that should never be linked by bare method name without type.
pub fn is_stdlib_method(fam: LangFamily, method_name: &str) -> bool {
    match fam {
        LangFamily::Rust => matches!(
            method_name,
            "get"
                | "parse"
                | "len"
                | "push"
                | "iter"
                | "map"
                | "insert"
                | "contains"
                | "clone"
                | "new"
                | "from"
                | "into"
                | "to_string"
                | "as_str"
                | "is_empty"
                | "pop"
                | "unwrap"
                | "expect"
                | "as_bytes"
                | "trim"
                | "split"
                | "collect"
        ),
        LangFamily::Python => matches!(
            method_name,
            "get"
                | "append"
                | "items"
                | "keys"
                | "split"
                | "join"
                | "format"
                | "pop"
                | "extend"
                | "values"
                | "strip"
                | "replace"
                | "startswith"
                | "endswith"
                | "update"
        ),
        LangFamily::Web => matches!(
            method_name,
            "get"
                | "set"
                | "push"
                | "map"
                | "filter"
                | "forEach"
                | "then"
                | "join"
                | "pop"
                | "find"
                | "includes"
                | "indexOf"
                | "split"
                | "slice"
                | "catch"
                | "trim"
        ),
    }
}

/// The extracted symbols, imports, and calls from a single source file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileScan {
    pub symbols: Vec<Symbol>,
    pub imports: Vec<Import>,
    pub calls: Vec<Call>,
}

/// Sanitizes a signature string into a single clean line truncated to 160 characters.
pub fn sanitize_signature(sig: &str) -> String {
    let single_line: String = sig
        .chars()
        .map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let trimmed = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.chars().count() > 160 {
        trimmed.chars().take(160).collect::<String>().trim_end().to_string()
    } else {
        trimmed
    }
}

/// Sanitizes a decorator string into a single clean line without newlines or tabs.
pub fn sanitize_decorator(dec: &str) -> String {
    let single_line: String = dec
        .chars()
        .map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    single_line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Dispatches scanning to the appropriate language scanner.
pub fn scan(lang: Lang, bytes: &[u8]) -> FileScan {
    #[cfg(test)]
    if bytes.starts_with(b"PANIC_TRIGGER") {
        panic!("scanner forced panic for test");
    }
    match lang {
        Lang::Rust => crate::lang_rust::scan_rust_file(bytes),
        Lang::Python => crate::lang_python::scan_python_file(bytes),
        Lang::Js | Lang::Ts => crate::lang_js::scan_js_file(lang, bytes),
        Lang::Html => crate::lang_html::scan_html_file(bytes),
    }
}

/// Dispatches import resolution to the appropriate language handler.
pub fn resolve_import(
    lang: Lang,
    current_file: &str,
    raw_import: &str,
    all_paths: &HashSet<String>,
    workspace_crates: &HashMap<String, String>,
) -> Option<String> {
    match lang {
        Lang::Rust => crate::lang_rust::resolve_import(current_file, raw_import, all_paths, workspace_crates),
        Lang::Python => crate::lang_python::resolve_import(current_file, raw_import, all_paths),
        Lang::Js | Lang::Ts => crate::lang_js::resolve_import(current_file, raw_import, all_paths),
        Lang::Html => crate::lang_html::resolve_import(current_file, raw_import, all_paths),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lang_of_detection() {
        assert_eq!(lang_of("src/lib.rs"), Some(Lang::Rust));
        assert_eq!(lang_of("app.py"), Some(Lang::Python));
        assert_eq!(lang_of("components/Card.tsx"), Some(Lang::Ts));
        assert_eq!(lang_of("utils.ts"), Some(Lang::Ts));
        assert_eq!(lang_of("server.js"), Some(Lang::Js));
        assert_eq!(lang_of("index.jsx"), Some(Lang::Js));
        assert_eq!(lang_of("templates/index.html"), Some(Lang::Html));
        assert_eq!(lang_of("page.htm"), Some(Lang::Html));
        assert_eq!(lang_of("README.md"), None);
        assert_eq!(lang_of("Cargo.toml"), None);
    }

    #[test]
    fn test_language_families() {
        assert_eq!(Lang::Rust.family(), LangFamily::Rust);
        assert_eq!(Lang::Python.family(), LangFamily::Python);
        assert_eq!(Lang::Js.family(), LangFamily::Web);
        assert_eq!(Lang::Ts.family(), LangFamily::Web);
        assert_eq!(Lang::Html.family(), LangFamily::Web);
    }

    #[test]
    fn test_sanitize_signature() {
        let raw = "fn saturate(\n\tsize: usize,\n\tceil: f32\n) -> usize";
        assert_eq!(sanitize_signature(raw), "fn saturate( size: usize, ceil: f32 ) -> usize");

        let long_sig = "a".repeat(200);
        assert_eq!(sanitize_signature(&long_sig).chars().count(), 160);
    }
}

