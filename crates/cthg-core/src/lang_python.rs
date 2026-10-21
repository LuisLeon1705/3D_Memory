//! Python language scanner for the project index.
//!
//! Extracts functions, classes, module-level variables and constants, imports
//! (including relative imports and module-level try/TYPE_CHECKING blocks),
//! and function/method calls without third-party dependencies or full AST parsing.

use crate::lang::{sanitize_decorator, sanitize_signature, Call, FileScan, Import, Symbol};
use std::collections::HashSet;

fn find_top_level_colon(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in s.char_indices() {
        if ch == '(' || ch == '[' || ch == '{' {
            depth += 1;
        } else if (ch == ')' || ch == ']' || ch == '}') && depth > 0 {
            depth -= 1;
        } else if ch == ':' && depth == 0 {
            return Some(idx);
        }
    }
    None
}

/// Strips comments and strings from Python source while preserving exact byte length,
/// newline layout, and indentation so line numbers and offsets stay 100% aligned.
pub fn strip_comments_and_strings(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let len = bytes.len();
    let mut i = 0;

    let mut paren_depth = 0usize;

    while i < len {
        let b = bytes[i];

        // 1. Line comment # (only if not in string)
        if b == b'#' {
            out.push(b' ');
            i += 1;
            while i < len && bytes[i] != b'\n' {
                out.push(if bytes[i] == b'\r' { b'\r' } else { b' ' });
                i += 1;
            }
            continue;
        }

        // 2. Track parentheses / brackets / braces for multiline expressions
        if b == b'(' || b == b'[' || b == b'{' {
            paren_depth += 1;
            out.push(b);
            i += 1;
            continue;
        } else if (b == b')' || b == b']' || b == b'}') && paren_depth > 0 {
            paren_depth -= 1;
            out.push(b);
            i += 1;
            continue;
        }

        // 3. String prefixes check: r, u, f, b, rb, br, rf, fr (case-insensitive)
        let mut quote_pos = i;
        let mut is_str_prefix = false;
        let ch0 = b.to_ascii_lowercase();
        if ch0 == b'r' || ch0 == b'u' || ch0 == b'f' || ch0 == b'b' {
            if i + 1 < len && (bytes[i + 1] == b'\'' || bytes[i + 1] == b'"') {
                is_str_prefix = true;
                quote_pos = i + 1;
            } else if i + 2 < len {
                let ch1 = bytes[i + 1].to_ascii_lowercase();
                if ((ch0 == b'r' && ch1 == b'b')
                    || (ch0 == b'b' && ch1 == b'r')
                    || (ch0 == b'r' && ch1 == b'f')
                    || (ch0 == b'f' && ch1 == b'r'))
                    && (bytes[i + 2] == b'\'' || bytes[i + 2] == b'"')
                {
                    is_str_prefix = true;
                    quote_pos = i + 2;
                }
            }
        } else if b == b'\'' || b == b'"' {
            is_str_prefix = true;
            quote_pos = i;
        }

        if is_str_prefix {
            let quote_char = bytes[quote_pos];
            let is_triple = quote_pos + 2 < len
                && bytes[quote_pos + 1] == quote_char
                && bytes[quote_pos + 2] == quote_char;

            // Blank out any prefix letters
            while i < quote_pos {
                out.push(b' ');
                i += 1;
            }

            if is_triple {
                // Triple quote string: """ or '''
                out.push(b' ');
                out.push(b' ');
                out.push(b' ');
                i += 3;
                while i < len {
                    if bytes[i] == quote_char
                        && i + 2 < len
                        && bytes[i + 1] == quote_char
                        && bytes[i + 2] == quote_char
                        && (i == 0 || bytes[i - 1] != b'\\')
                    {
                        out.push(b' ');
                        out.push(b' ');
                        out.push(b' ');
                        i += 3;
                        break;
                    } else {
                        out.push(if bytes[i] == b'\n' {
                            b'\n'
                        } else if bytes[i] == b'\r' {
                            b'\r'
                        } else {
                            b' '
                        });
                        i += 1;
                    }
                }
            } else {
                // Single line string: '...' or "..."
                out.push(b' ');
                i += 1;
                while i < len {
                    if bytes[i] == quote_char && (i == 0 || bytes[i - 1] != b'\\') {
                        out.push(b' ');
                        i += 1;
                        break;
                    } else if bytes[i] == b'\n' {
                        out.push(b'\n');
                        i += 1;
                        break;
                    } else {
                        out.push(if bytes[i] == b'\r' { b'\r' } else { b' ' });
                        i += 1;
                    }
                }
            }
            continue;
        }

        out.push(b);
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}


fn split_eq_depth0(s: &str) -> Option<(&str, &str)> {
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    for (i, c) in s.char_indices() {
        if paren == 0 && bracket == 0 && brace == 0 && c == '=' {
            if s[i..].starts_with("==") {
                return None;
            }
            return Some((&s[..i], &s[i + 1..]));
        }
        match c {
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            '[' => bracket += 1,
            ']' => bracket = bracket.saturating_sub(1),
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            _ => {}
        }
    }
    None
}

fn split_colon_depth0(s: &str) -> Option<(&str, &str)> {
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    for (i, c) in s.char_indices() {
        if paren == 0 && bracket == 0 && brace == 0 && c == ':' {
            return Some((&s[..i], &s[i + 1..]));
        }
        match c {
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            '[' => bracket += 1,
            ']' => bracket = bracket.saturating_sub(1),
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            _ => {}
        }
    }
    None
}

pub fn scan_python_file(bytes: &[u8]) -> FileScan {
    let clean = strip_comments_and_strings(bytes);
    scan_python(&clean)
}

#[derive(Clone, Debug)]
struct ScopeItem {
    indent: usize,
    class_name: Option<String>,
    is_fn: bool,
    sym_idx: Option<usize>,
}

pub fn scan_python(source: &str) -> FileScan {
    let mut scan = FileScan::default();
    let lines: Vec<&str> = source.lines().collect();
    let total_lines = lines.len();

    // 1. First pass: find __all__ = [...] if present
    let mut all_exports: Option<HashSet<String>> = None;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("__all__") && trimmed.contains('=') {
            let mut names = HashSet::new();
            if let Some((_, rest)) = trimmed.split_once('=') {
                for part in rest.split([',', '[', ']', '(', ')', '"', '\'']) {
                    let p = part.trim();
                    if !p.is_empty() && is_ident(p) {
                        names.insert(p.to_string());
                    }
                }
            }
            all_exports = Some(names);
            break;
        }
    }

    let mut scope_stack: Vec<ScopeItem> = Vec::new();
    let mut pending_decorators: Vec<String> = Vec::new();

    // Track if we are inside `if __name__ == "__main__":`
    let mut main_block_indent: Option<usize> = None;

    let mut line_idx = 0;
    while line_idx < total_lines {
        let line_num = line_idx + 1;
        let line = lines[line_idx];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            line_idx += 1;
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Pop scopes that are deeper or equal to current indent
        while let Some(top) = scope_stack.last() {
            if indent <= top.indent {
                if let Some(s_idx) = top.sym_idx {
                    if scan.symbols[s_idx].end_line == scan.symbols[s_idx].line && line_num > 1 {
                        scan.symbols[s_idx].end_line = line_num - 1;
                    }
                }
                scope_stack.pop();
            } else {
                break;
            }
        }

        if let Some(m_indent) = main_block_indent {
            if indent <= m_indent {
                main_block_indent = None;
            }
        }

        // Decorators: `@...`
        if trimmed.starts_with('@') {
            let dec_text = sanitize_decorator(trimmed);
            if !dec_text.is_empty() {
                pending_decorators.push(dec_text);
            }
            line_idx += 1;
            continue;
        }

        // Module-level `if __name__ == ...`
        if trimmed.starts_with("if __name__") && trimmed.contains("__main__") {
            main_block_indent = Some(indent);
            line_idx += 1;
            continue;
        }

        // Module-level try / except / if TYPE_CHECKING
        if trimmed.starts_with("try:") || trimmed.starts_with("if TYPE_CHECKING:") || trimmed.starts_with("if typing.TYPE_CHECKING:") {
            // These don't create function/class scopes, and imports inside them still count
            line_idx += 1;
            continue;
        }

        // Imports
        if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
            let mut import_raw = trimmed.to_string();
            let mut j = line_idx;

            // Multiline import if parentheses are unclosed
            let mut open_p = import_raw.matches('(').count();
            let mut close_p = import_raw.matches(')').count();
            while open_p > close_p && j + 1 < total_lines {
                j += 1;
                import_raw.push(' ');
                import_raw.push_str(lines[j].trim());
                open_p = import_raw.matches('(').count();
                close_p = import_raw.matches(')').count();
            }

            for imp in parse_py_import(&import_raw) {
                scan.imports.push(Import {
                    raw: imp,
                    resolved: None,
                });
            }

            line_idx = j + 1;
            continue;
        }

        // Class definition: `class Name(Bases):`
        if trimmed.starts_with("class ") {
            let rest = trimmed.strip_prefix("class ").unwrap_or("").trim();
            let mut full_header = rest.to_string();
            let mut j = line_idx;
            while find_top_level_colon(&full_header).is_none() && j + 1 < total_lines {
                j += 1;
                full_header.push(' ');
                full_header.push_str(lines[j].trim());
            }

            if let Some(col_pos) = find_top_level_colon(&full_header) {
                let header_part = &full_header[..col_pos];
                let header = header_part.trim();
                let (class_name, bases) = if let Some((name_part, base_part)) = header.split_once('(') {
                    let name = name_part.trim();
                    let b = format!("({}", base_part.trim());
                    (name, b)
                } else {
                    (header, String::new())
                };

                if is_ident(class_name) {
                    let is_public = is_name_public(class_name, all_exports.as_ref());
                    let is_test = class_name.starts_with("Test");
                    let decorators = std::mem::take(&mut pending_decorators);
                    let sig = sanitize_signature(&bases);

                    let sym_idx = scan.symbols.len();
                    scan.symbols.push(Symbol {
                        name: class_name.to_string(),
                        kind: "class".to_string(),
                        line: line_num,
                        end_line: line_num,
                        public: is_public,
                        parent: current_class(&scope_stack),
                        test: is_test,
                        signature: sig,
                        decorators,
                    });

                    scope_stack.push(ScopeItem {
                        indent,
                        class_name: Some(class_name.to_string()),
                        is_fn: false,
                        sym_idx: Some(sym_idx),
                    });
                }
            }
            line_idx = j + 1;
            continue;
        }

        // Function definition: `def name(...) -> Ret:` or `async def name(...) -> Ret:`
        let is_async = trimmed.starts_with("async def ");
        if trimmed.starts_with("def ") || is_async {
            let prefix = if is_async { "async def " } else { "def " };
            let rest = trimmed.strip_prefix(prefix).unwrap_or("").trim();
            let mut full_header = rest.to_string();
            let mut j = line_idx;
            while find_top_level_colon(&full_header).is_none() && j + 1 < total_lines {
                j += 1;
                full_header.push(' ');
                full_header.push_str(lines[j].trim());
            }

            if let Some(col_pos) = find_top_level_colon(&full_header) {
                let header_part = &full_header[..col_pos];
                let header = header_part.trim();
                let (fn_name, sig_part) = if let Some(pos) = header.find('(') {
                    (header[..pos].trim(), &header[pos..])
                } else {
                    (header, "")
                };

                // Check if inside another function: nested functions do not count
                let is_nested_fn = scope_stack.iter().any(|s| s.is_fn);

                if is_ident(fn_name) && !is_nested_fn {
                    let is_public = is_name_public(fn_name, all_exports.as_ref());
                    let is_test = fn_name.starts_with("test_");
                    let parent = current_class(&scope_stack);
                    let decorators = std::mem::take(&mut pending_decorators);
                    let sig = sanitize_signature(sig_part);

                    let sym_idx = scan.symbols.len();
                    scan.symbols.push(Symbol {
                        name: fn_name.to_string(),
                        kind: "fn".to_string(),
                        line: line_num,
                        end_line: line_num,
                        public: is_public,
                        parent,
                        test: is_test,
                        signature: sig,
                        decorators,
                    });

                    scope_stack.push(ScopeItem {
                        indent,
                        class_name: None,
                        is_fn: true,
                        sym_idx: Some(sym_idx),
                    });
                } else if is_ident(fn_name) && is_nested_fn {
                    // Push nested scope so inner indents are properly tracked
                    scope_stack.push(ScopeItem {
                        indent,
                        class_name: None,
                        is_fn: true,
                        sym_idx: None,
                    });
                }
            }
            line_idx = j + 1;
            continue;
        }

        // Clear any orphaned decorators if no def/class followed
        pending_decorators.clear();

        // Module-level assignments (indent 0, outside main block)
        if indent == 0 && main_block_indent.is_none() && scope_stack.is_empty() {
            if let Some((lhs, _rhs)) = split_eq_depth0(trimmed) {
                let var_name = split_colon_depth0(lhs).map(|(n, _)| n).unwrap_or(lhs).trim();
                if is_ident(var_name) && !var_name.starts_with("if ") {
                    let is_public = is_name_public(var_name, all_exports.as_ref());
                    let is_const = var_name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
                    let kind = if is_const { "const" } else { "var" };
                    let sig = if let Some((_, typ)) = split_colon_depth0(lhs) {
                        sanitize_signature(&format!(": {}", typ.trim()))
                    } else {
                        String::new()
                    };

                    scan.symbols.push(Symbol {
                        name: var_name.to_string(),
                        kind: kind.to_string(),
                        line: line_num,
                        end_line: line_num,
                        public: is_public,
                        parent: None,
                        test: false,
                        signature: sig,
                        decorators: Vec::new(),
                    });
                }
            }
        }

        // Calls extraction: `name(` or `obj.name(`
        let caller_name = current_caller(&scope_stack);
        for call_name in extract_calls(trimmed) {
            scan.calls.push(Call {
                from: caller_name.clone(),
                name: call_name,
                line: line_num,
            });
        }

        line_idx += 1;
    }

    // Set end_line for unclosed symbols at EOF
    for item in scope_stack {
        if let Some(s_idx) = item.sym_idx {
            if scan.symbols[s_idx].end_line == scan.symbols[s_idx].line && total_lines >= scan.symbols[s_idx].line {
                scan.symbols[s_idx].end_line = total_lines;
            }
        }
    }

    scan
}



fn current_class(stack: &[ScopeItem]) -> Option<String> {
    stack.iter().rev().find_map(|s| s.class_name.clone())
}

fn current_caller(stack: &[ScopeItem]) -> String {
    let mut parts = Vec::new();
    for s in stack {
        if let Some(ref c) = s.class_name {
            parts.push(c.clone());
        }
    }
    if let Some(top) = stack.last() {
        if top.is_fn {
            if let Some(idx) = top.sym_idx {
                // Return Class::method or method
                if parts.is_empty() {
                    return format!("fn_{}", idx);
                }
            }
        }
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("::")
    }
}

fn is_name_public(name: &str, all_exports: Option<&HashSet<String>>) -> bool {
    if let Some(all) = all_exports {
        all.contains(name)
    } else {
        !name.starts_with('_') || (name.starts_with("__") && name.ends_with("__"))
    }
}

fn is_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Parses Python import statements:
/// `import foo, bar as b` -> ["import foo", "import bar"]
/// `from foo.bar import a, b` -> ["from foo.bar import a", "from foo.bar import b"]
/// `from . import a` -> ["from . import a"]
pub fn parse_py_import(raw: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let clean = raw.replace(['(', ')', '\n', '\r'], " ");
    let s = clean.trim();

    if let Some(rest) = s.strip_prefix("import ") {
        for part in rest.split(',') {
            let part = part.trim();
            let base_mod = part.split_whitespace().next().unwrap_or("").trim();
            if !base_mod.is_empty() {
                imports.push(format!("import {base_mod}"));
            }
        }
    } else if let Some(rest) = s.strip_prefix("from ") {
        if let Some((mod_part, names_part)) = rest.split_once(" import ") {
            let from_mod = mod_part.trim();
            for name in names_part.split(',') {
                let name = name.split_whitespace().next().unwrap_or("").trim();
                if !name.is_empty() {
                    imports.push(format!("from {from_mod} import {name}"));
                }
            }
        }
    }

    imports
}

/// Extracts call names `foo(` and `obj.foo(` while skipping Python keywords.
fn extract_calls(line: &str) -> Vec<String> {
    let mut calls = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;

    const PY_KEYWORDS: &[&str] = &[
        "if", "elif", "for", "while", "with", "return", "def", "class",
        "import", "from", "raise", "try", "except", "finally", "assert",
        "yield", "match", "case", "lambda", "print", "and", "or", "not", "in", "is",
    ];

    while i < n {
        if chars[i] == '(' && i > 0 {
            // Read identifier backwards from before '('
            let mut j = i;
            while j > 0 && chars[j - 1].is_whitespace() {
                j -= 1;
            }
            let end_ident = j;
            while j > 0 && (chars[j - 1].is_alphanumeric() || chars[j - 1] == '_') {
                j -= 1;
            }
            if end_ident > j {
                let name: String = chars[j..end_ident].iter().collect();
                if is_ident(&name) && !PY_KEYWORDS.contains(&name.as_str()) && !calls.contains(&name) {
                    calls.push(name);
                }
            }
        }
        i += 1;
    }

    calls
}

/// Resolves a Python import statement against known repository files.
pub fn resolve_import(
    current_file: &str,
    raw_import: &str,
    all_paths: &HashSet<String>,
) -> Option<String> {
    let raw = raw_import.trim();

    // 1. `from .pkg import mod` or `from . import mod`
    if let Some(rest) = raw.strip_prefix("from ") {
        if let Some((mod_target, sym_target)) = rest.split_once(" import ") {
            let mod_target = mod_target.trim();
            let sym_target = sym_target.trim();

            if mod_target.starts_with('.') {
                // Relative import
                let norm_cur = current_file.replace('\\', "/");
                let cur_dir = norm_cur.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                let dots = mod_target.chars().take_while(|c| *c == '.').count();
                let sub_path = &mod_target[dots..];

                let mut base_dir = cur_dir.to_string();
                for _ in 1..dots {
                    base_dir = base_dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("").to_string();
                }

                // Check mod_target/sym_target.py or mod_target.py
                let path_parts = if sub_path.is_empty() {
                    sym_target.replace('.', "/")
                } else {
                    format!("{}/{}", sub_path.replace('.', "/"), sym_target)
                };

                let cand1 = if base_dir.is_empty() {
                    format!("{path_parts}.py")
                } else {
                    format!("{base_dir}/{path_parts}.py")
                };
                if all_paths.contains(&cand1) {
                    return Some(cand1);
                }

                let cand2 = if base_dir.is_empty() {
                    format!("{path_parts}/__init__.py")
                } else {
                    format!("{base_dir}/{path_parts}/__init__.py")
                };
                if all_paths.contains(&cand2) {
                    return Some(cand2);
                }

                // Or maybe sub_path itself was the module
                if !sub_path.is_empty() {
                    let cand3 = if base_dir.is_empty() {
                        format!("{}.py", sub_path.replace('.', "/"))
                    } else {
                        format!("{}/{}.py", base_dir, sub_path.replace('.', "/"))
                    };
                    if all_paths.contains(&cand3) {
                        return Some(cand3);
                    }
                }

                // If sym_target is imported from package __init__.py
                let init_cand = if base_dir.is_empty() {
                    "__init__.py".to_string()
                } else {
                    format!("{base_dir}/__init__.py")
                };
                if all_paths.contains(&init_cand) {
                    return Some(init_cand);
                }

                return None;
            } else {
                // Absolute import `from a.b import c`
                let target_path = mod_target.replace('.', "/");
                let sym_path = format!("{}/{}", target_path, sym_target);

                if let Some(res) = resolve_py_target(&sym_path, all_paths) {
                    return Some(res);
                }
                if let Some(res) = resolve_py_target(&target_path, all_paths) {
                    return Some(res);
                }
            }
        }
    } else if let Some(rest) = raw.strip_prefix("import ") {
        let mod_target = rest.split_whitespace().next().unwrap_or("").trim();
        let target_path = mod_target.replace('.', "/");
        if let Some(res) = resolve_py_target(&target_path, all_paths) {
            return Some(res);
        }
    }

    None
}

fn resolve_py_target(target: &str, all_paths: &HashSet<String>) -> Option<String> {
    // 1. Direct from root
    let cand1 = format!("{target}.py");
    if all_paths.contains(&cand1) {
        return Some(cand1);
    }
    let cand2 = format!("{target}/__init__.py");
    if all_paths.contains(&cand2) {
        return Some(cand2);
    }

    // 2. From src/
    let cand_src1 = format!("src/{target}.py");
    if all_paths.contains(&cand_src1) {
        return Some(cand_src1);
    }
    let cand_src2 = format!("src/{target}/__init__.py");
    if all_paths.contains(&cand_src2) {
        return Some(cand_src2);
    }

    // 3. Across any package root folder in the repo
    for p in all_paths {
        if p.ends_with(&format!("/{target}.py")) {
            return Some(p.clone());
        }
        if p.ends_with(&format!("/{target}/__init__.py")) {
            return Some(p.clone());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_symbols_and_indentation() {
        let py = r#"
class BaseServer:
    def __init__(self, port: int = 8080) -> None:
        self.port = port

    def start(self):
        def helper():
            pass
        self.run()

def top_level(x, y: int = 1) -> bool:
    return True

MAX_RETRIES = 3
app = None
"#;
        let scan = scan_python(py);
        assert_eq!(scan.symbols.len(), 6);
        assert_eq!(scan.symbols[5].name, "app");
        assert_eq!(scan.symbols[5].kind, "var");

        // class BaseServer
        assert_eq!(scan.symbols[0].name, "BaseServer");
        assert_eq!(scan.symbols[0].kind, "class");
        assert_eq!(scan.symbols[0].line, 2);

        // def __init__
        assert_eq!(scan.symbols[1].name, "__init__");
        assert_eq!(scan.symbols[1].kind, "fn");
        assert_eq!(scan.symbols[1].parent.as_deref(), Some("BaseServer"));
        assert_eq!(scan.symbols[1].signature, "(self, port: int = 8080) -> None");

        // def start
        assert_eq!(scan.symbols[2].name, "start");
        assert_eq!(scan.symbols[2].kind, "fn");
        assert_eq!(scan.symbols[2].parent.as_deref(), Some("BaseServer"));

        // helper inside start is nested: NOT a symbol!

        // def top_level
        assert_eq!(scan.symbols[3].name, "top_level");
        assert_eq!(scan.symbols[3].parent, None);
        assert_eq!(scan.symbols[3].signature, "(x, y: int = 1) -> bool");

        // const MAX_RETRIES
        assert_eq!(scan.symbols[4].name, "MAX_RETRIES");
        assert_eq!(scan.symbols[4].kind, "const");
    }

    #[test]
    fn test_python_decorators_and_multiline() {
        let py = r#"
@app.route("/api/v1/user", methods=["GET", "POST"])
@login_required
def get_user(
    request: Request,
    user_id: str,
) -> UserResponse:
    return fetch(user_id)
"#;
        let scan = scan_python(py);
        assert_eq!(scan.symbols.len(), 1);
        let s = &scan.symbols[0];
        assert_eq!(s.name, "get_user");
        assert_eq!(s.decorators.len(), 2);
        assert_eq!(s.decorators[0], "@app.route(\"/api/v1/user\", methods=[\"GET\", \"POST\"])");
        assert_eq!(s.decorators[1], "@login_required");
        assert_eq!(s.signature, "( request: Request, user_id: str, ) -> UserResponse");
    }

    #[test]
    fn test_python_all_controls_visibility() {
        let py = r#"
__all__ = ["public_func", "_explicit_public"]

def public_func():
    pass

def other_func():
    pass

def _explicit_public():
    pass
"#;
        let scan = scan_python(py);
        let pub_f = scan.symbols.iter().find(|s| s.name == "public_func").unwrap();
        assert!(pub_f.public);
        let oth_f = scan.symbols.iter().find(|s| s.name == "other_func").unwrap();
        assert!(!oth_f.public);
        let exp_f = scan.symbols.iter().find(|s| s.name == "_explicit_public").unwrap();
        assert!(exp_f.public);
    }

    #[test]
    fn test_python_try_and_type_checking_imports_count() {
        let py = r#"
try:
    import ujson as json
except ImportError:
    import json

if TYPE_CHECKING:
    from typing import Optional, List
"#;
        let scan = scan_python(py);
        let raw_imps: Vec<_> = scan.imports.iter().map(|i| i.raw.as_str()).collect();
        assert!(raw_imps.contains(&"import ujson"));
        assert!(raw_imps.contains(&"import json"));
        assert!(raw_imps.contains(&"from typing import Optional"));
        assert!(raw_imps.contains(&"from typing import List"));
    }

    #[test]
    fn test_python_if_name_main_assignments_not_symbols() {
        let py = r#"
CONFIG_PATH = "config.yaml"

if __name__ == "__main__":
    test_val = 123
    run_server()
"#;
        let scan = scan_python(py);
        assert_eq!(scan.symbols.len(), 1);
        assert_eq!(scan.symbols[0].name, "CONFIG_PATH");
        assert!(!scan.symbols.iter().any(|s| s.name == "test_val"));
    }

    #[test]
    fn test_python_relative_and_absolute_import_resolution() {
        let mut paths = HashSet::new();
        paths.insert("pkg/util.py".to_string());
        paths.insert("pkg/sub/worker.py".to_string());
        paths.insert("pkg/sub/__init__.py".to_string());

        let res1 = resolve_import("pkg/sub/worker.py", "from ..util import helper", &paths);
        assert_eq!(res1, Some("pkg/util.py".to_string()));

        let res2 = resolve_import("pkg/sub/worker.py", "from . import constants", &paths);
        assert_eq!(res2, Some("pkg/sub/__init__.py".to_string()));

        let res3 = resolve_import("app.py", "from pkg.util import helper", &paths);
        assert_eq!(res3, Some("pkg/util.py".to_string()));

        let res_ext = resolve_import("app.py", "import requests", &paths);
        assert_eq!(res_ext, None);
    }

    #[test]
    fn python_const_and_var_strip_assignment_and_value() {
        let code = r#"
TIMEOUT: int = 30
DEBUG = True
CACHE: dict[str, list[int]] = {}
"#;
        let scan = scan_python_file(code.as_bytes());
        let timeout_sym = scan.symbols.iter().find(|s| s.name == "TIMEOUT").unwrap();
        assert_eq!(timeout_sym.kind, "const");
        assert_eq!(timeout_sym.signature, ": int");

        let debug_sym = scan.symbols.iter().find(|s| s.name == "DEBUG").unwrap();
        assert_eq!(debug_sym.kind, "const");
        assert_eq!(debug_sym.signature, "");

        let cache_sym = scan.symbols.iter().find(|s| s.name == "CACHE").unwrap();
        assert_eq!(cache_sym.kind, "const");
        assert_eq!(cache_sym.signature, ": dict[str, list[int]]");
    }
}
