//! Rust language scanner for the project index.
//!
//! Extracts top-level declarations (functions, methods with parent types, structs, enums,
//! traits, types, constants, statics, modules, macro_rules), module and use imports,
//! and function call references without third-party dependencies or full AST parsing.

pub use crate::lang::{sanitize_signature, Call, FileScan, Import, Symbol};
use std::collections::{HashMap, HashSet};

/// Strips comments and string literals from Rust source code while preserving exact byte length
/// and newline layout so line numbers and offsets stay 100% aligned with the original file.
pub fn scan_rust_file(bytes: &[u8]) -> FileScan {
    let clean = strip_comments_and_strings(bytes);
    scan_rust(&clean)
}

pub fn strip_comments_and_strings(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // 1. Line comment //
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            while i < len && bytes[i] != b'\n' {
                out.push(if bytes[i] == b'\r' { b'\r' } else { b' ' });
                i += 1;
            }
            continue;
        }

        // 2. Block comment /* ... */ with nesting
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            let mut depth = 1usize;
            while i < len && depth > 0 {
                if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                    depth += 1;
                } else if bytes[i] == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                    depth -= 1;
                } else {
                    out.push(if bytes[i] == b'\n' { b'\n' } else if bytes[i] == b'\r' { b'\r' } else { b' ' });
                    i += 1;
                }
            }
            continue;
        }

        // 3. Raw strings: r"...", r#"..."#, r##"..."##, br#"..."#, c#"..."#
        let is_raw_start = (b == b'r' && i + 1 < len && (bytes[i + 1] == b'"' || bytes[i + 1] == b'#'))
            || ((b == b'b' || b == b'c')
                && i + 2 < len
                && bytes[i + 1] == b'r'
                && (bytes[i + 2] == b'"' || bytes[i + 2] == b'#'));

        if is_raw_start {
            let start = i;
            let r_pos = if b == b'r' { i } else { i + 1 };
            let mut h = r_pos + 1;
            let mut hashes = 0usize;
            while h < len && bytes[h] == b'#' {
                hashes += 1;
                h += 1;
            }
            if h < len && bytes[h] == b'"' {
                // It is a valid raw string opener
                out.resize(out.len() + (h - start + 1), b' ');
                i = h + 1;
                let mut closed = false;
                while i < len {
                    if bytes[i] == b'"' {
                        let mut match_hashes = 0usize;
                        while i + 1 + match_hashes < len && bytes[i + 1 + match_hashes] == b'#' && match_hashes < hashes {
                            match_hashes += 1;
                        }
                        if match_hashes == hashes {
                            out.resize(out.len() + (hashes + 1), b' ');
                            i += 1 + hashes;
                            closed = true;
                            break;
                        }
                    }
                    out.push(if bytes[i] == b'\n' { b'\n' } else if bytes[i] == b'\r' { b'\r' } else { b' ' });
                    i += 1;
                }
                if closed {
                    continue;
                }
                break;
            }
        }

        // 4. Regular strings: "...", b"...", c"..."
        let is_str_start = b == b'"' || ((b == b'b' || b == b'c') && i + 1 < len && bytes[i + 1] == b'"');
        if is_str_start {
            let start = i;
            let quote_pos = if b == b'"' { i } else { i + 1 };
            out.resize(out.len() + (quote_pos - start + 1), b' ');
            i = quote_pos + 1;
            while i < len {
                let cb = bytes[i];
                if cb == b'\\' {
                    out.push(b' ');
                    out.push(if i + 1 < len && bytes[i + 1] == b'\n' { b'\n' } else { b' ' });
                    i += 2;
                } else if cb == b'"' {
                    out.push(b' ');
                    i += 1;
                    break;
                } else {
                    out.push(if cb == b'\n' { b'\n' } else if cb == b'\r' { b'\r' } else { b' ' });
                    i += 1;
                }
            }
            continue;
        }

        // 5. Character literals vs lifetimes: 'a', '\'', '\n', b'x', vs 'lifetime
        let is_char_start = b == b'\'' || (b == b'b' && i + 1 < len && bytes[i + 1] == b'\'');
        if is_char_start {
            let quote_pos = if b == b'\'' { i } else { i + 1 };
            // Check if this looks like a char literal: closed by a single quote on the same line
            if quote_pos + 1 < len {
                let next = bytes[quote_pos + 1];
                let mut char_end = None;
                if next == b'\\' {
                    // Escaped char: find closing quote within 12 bytes on the same line
                    let mut k = quote_pos + 2;
                    while k < len && k <= quote_pos + 12 && bytes[k] != b'\n' {
                        if bytes[k] == b'\'' {
                            char_end = Some(k);
                            break;
                        }
                        k += 1;
                    }
                } else if next != b'\'' && quote_pos + 2 < len && bytes[quote_pos + 2] == b'\'' {
                    char_end = Some(quote_pos + 2);
                }

                if let Some(end_pos) = char_end {
                    out.resize(out.len() + (end_pos - i + 1), b' ');
                    i = end_pos + 1;
                    continue;
                }
            }
        }

        out.push(b);
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

/// Token in stripped Rust source code.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    text: String,
    line: usize,
    offset: usize,
}

fn tokenize(s: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let num_chars = chars.len();
    let mut c_idx = 0;
    let mut line = 1usize;

    while c_idx < num_chars {
        let (byte_pos, ch) = chars[c_idx];

        if ch == '\n' {
            line += 1;
            c_idx += 1;
            continue;
        }
        if ch.is_whitespace() {
            c_idx += 1;
            continue;
        }

        // Identifier or keyword
        if ch == '_' || ch.is_alphabetic() {
            let start = byte_pos;
            while c_idx < num_chars && (chars[c_idx].1 == '_' || chars[c_idx].1.is_alphanumeric()) {
                c_idx += 1;
            }
            let end = if c_idx < num_chars { chars[c_idx].0 } else { s.len() };
            tokens.push(Token {
                text: s[start..end].to_string(),
                line,
                offset: start,
            });
            continue;
        }

        // Lifetime 'ident
        if ch == '\'' && c_idx + 1 < num_chars && (chars[c_idx + 1].1 == '_' || chars[c_idx + 1].1.is_alphabetic()) {
            let start = byte_pos;
            c_idx += 1;
            while c_idx < num_chars && (chars[c_idx].1 == '_' || chars[c_idx].1.is_alphanumeric()) {
                c_idx += 1;
            }
            let end = if c_idx < num_chars { chars[c_idx].0 } else { s.len() };
            tokens.push(Token {
                text: s[start..end].to_string(),
                line,
                offset: start,
            });
            continue;
        }

        // Multi-character punctuation: ::
        if ch == ':' && c_idx + 1 < num_chars && chars[c_idx + 1].1 == ':' {
            tokens.push(Token {
                text: "::".to_string(),
                line,
                offset: byte_pos,
            });
            c_idx += 2;
            continue;
        }

        // Single character punctuation
        tokens.push(Token {
            text: ch.to_string(),
            line,
            offset: byte_pos,
        });
        c_idx += 1;
    }

    tokens
}

#[derive(Clone, Debug)]
enum Scope {
    Root,
    Mod { is_test: bool },
    Impl { target: String, is_test: bool },
    Trait { name: String, is_test: bool },
    Fn { name: String, is_test: bool },
    Other { is_test: bool },
}

impl Scope {
    fn is_test(&self) -> bool {
        match self {
            Scope::Root => false,
            Scope::Mod { is_test }
            | Scope::Impl { is_test, .. }
            | Scope::Trait { is_test, .. }
            | Scope::Fn { is_test, .. }
            | Scope::Other { is_test } => *is_test,
        }
    }
}

fn advance_sig_tokens(tokens: &[Token], start: usize) -> (usize, Option<usize>) {
    let n = tokens.len();
    let mut j = start;
    let mut paren: i32 = 0;
    let mut bracket: i32 = 0;
    let mut brace: i32 = 0;
    let mut angle: i32 = 0;
    let mut eq_pos: Option<usize> = None;

    while j < n {
        let t = tokens[j].text.as_str();
        if paren == 0 && bracket == 0 && brace == 0 && angle == 0 {
            if t == "{" || t == ";" {
                break;
            }
            if t == "=" && eq_pos.is_none() {
                eq_pos = Some(j);
            }
        }
        match t {
            "(" => paren += 1,
            ")" => if paren > 0 { paren -= 1; },
            "[" => bracket += 1,
            "]" => if bracket > 0 { bracket -= 1; },
            "{" => brace += 1,
            "}" => if brace > 0 { brace -= 1; },
            "<" => angle += 1,
            ">" => {
                if j > start && tokens[j - 1].text == "-" {
                    // Part of `->` arrow, not a closing generic angle
                } else if angle > 0 {
                    angle -= 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    (j, eq_pos)
}

pub fn scan_rust(content: &str) -> FileScan {
    let clean = strip_comments_and_strings(content.as_bytes());
    let tokens = tokenize(&clean);
    let mut scan = FileScan::default();

    let mut stack: Vec<(Scope, Option<usize>)> = vec![(Scope::Root, None)];
    let mut pending_pub = false;
    let mut pending_cfg_test = false;
    let mut pending_test_attr = false;

    let n = tokens.len();
    let mut i = 0;

    while i < n {
        let tok = &tokens[i];

        // 1. Attributes: #[cfg(test)], #[test]
        if tok.text == "#" && i + 1 < n && tokens[i + 1].text == "[" {
            let mut j = i + 2;
            let mut attr_tokens = Vec::new();
            while j < n && tokens[j].text != "]" {
                attr_tokens.push(tokens[j].text.as_str());
                j += 1;
            }
            if attr_tokens.contains(&"test") {
                if attr_tokens.contains(&"cfg") {
                    pending_cfg_test = true;
                } else {
                    pending_test_attr = true;
                }
            }
            i = if j < n { j + 1 } else { n };
            continue;
        }

        // 2. Visibility: pub, pub(crate), pub(super), pub(...)
        if tok.text == "pub" {
            pending_pub = true;
            i += 1;
            if i < n && tokens[i].text == "(" {
                let mut depth = 1usize;
                i += 1;
                while i < n && depth > 0 {
                    if tokens[i].text == "(" {
                        depth += 1;
                    } else if tokens[i].text == ")" {
                        depth -= 1;
                    }
                    i += 1;
                }
            }
            continue;
        }

        // 3. Imports: `use ...;`
        if tok.text == "use" && matches!(stack.last().map(|(s, _)| s), Some(Scope::Root | Scope::Mod { .. })) {
            let _line = tok.line;
            let mut j = i + 1;
            let mut use_tokens = Vec::new();
            while j < n && tokens[j].text != ";" && tokens[j].text != "{" {
                use_tokens.push(tokens[j].text.as_str());
                j += 1;
            }
            if j < n && tokens[j].text == "{" {
                // Collect braced use tree until matching closing } and ;
                let mut brace_depth = 1usize;
                let mut tree_tokens = use_tokens.clone();
                tree_tokens.push("{");
                j += 1;
                while j < n && brace_depth > 0 {
                    if tokens[j].text == "{" {
                        brace_depth += 1;
                    } else if tokens[j].text == "}" {
                        brace_depth -= 1;
                    }
                    tree_tokens.push(tokens[j].text.as_str());
                    j += 1;
                }
                while j < n && tokens[j].text != ";" {
                    j += 1;
                }
                let full_use_expr = tree_tokens.join("");
                for path in expand_use_expr(&full_use_expr) {
                    scan.imports.push(Import {
                        raw: path,
                        resolved: None,
                    });
                }
            } else {
                let raw_path = use_tokens.join("");
                // Strip trailing `as alias` if present
                let path = if let Some(pos) = raw_path.find("as") {
                    raw_path[..pos].trim().to_string()
                } else {
                    raw_path
                };
                if !path.is_empty() {
                    scan.imports.push(Import {
                        raw: path,
                        resolved: None,
                    });
                }
            }
            i = if j < n { j + 1 } else { n };
            pending_pub = false;
            pending_test_attr = false;
            continue;
        }

        // 4. `mod foo;` or `mod foo { ... }`
        if tok.text == "mod" && i + 1 < n && is_ident(&tokens[i + 1].text) {
            let mod_name = tokens[i + 1].text.clone();
            let line = tok.line;
            let is_test = pending_cfg_test || mod_name == "tests" || stack.iter().any(|(s, _)| s.is_test());
            pending_cfg_test = false;

            if i + 2 < n && tokens[i + 2].text == ";" {
                // mod foo; declaration -> acts as symbol and import
                scan.symbols.push(Symbol {
                    name: mod_name.clone(),
                    kind: "mod".to_string(),
                    line,
                    end_line: tokens[i + 2].line,
                    public: pending_pub,
                    parent: None,
                    test: is_test,
                    signature: String::new(),
                    decorators: Vec::new(),
                });
                scan.imports.push(Import {
                    raw: format!("mod {mod_name};"),
                    resolved: None,
                });
                pending_pub = false;
                pending_test_attr = false;
                i += 3;
                continue;
            } else if i + 2 < n && (tokens[i + 2].text == "{" || (i + 3 < n && tokens[i + 3].text == "{")) {
                let sym_idx = scan.symbols.len();
                scan.symbols.push(Symbol {
                    name: mod_name.clone(),
                    kind: "mod".to_string(),
                    line,
                    end_line: line,
                    public: pending_pub,
                    parent: None,
                    test: is_test,
                    signature: String::new(),
                    decorators: Vec::new(),
                });
                let mut brace_idx = i + 2;
                while brace_idx < n && tokens[brace_idx].text != "{" {
                    brace_idx += 1;
                }
                stack.push((Scope::Mod { is_test }, Some(sym_idx)));
                pending_pub = false;
                pending_test_attr = false;
                i = brace_idx + 1;
                continue;
            }
        }

        // 5. Declarations: struct, enum, trait, type, const, static
        if matches!(tok.text.as_str(), "struct" | "enum" | "trait" | "type" | "const" | "static") {
            let kind = tok.text.clone();
            let line = tok.line;
            let is_test = pending_test_attr || stack.iter().any(|(s, _)| s.is_test());
            pending_test_attr = false;

            let (name_idx, sig_start_idx) = if kind == "static" && i + 1 < n && tokens[i + 1].text == "mut" {
                (i + 2, i + 3)
            } else {
                (i + 1, i + 2)
            };

            if name_idx < n && is_ident(&tokens[name_idx].text) {
                let name = tokens[name_idx].text.clone();
                let sym_idx = scan.symbols.len();
                let parent = current_parent(&stack);

                let (j, eq_pos) = advance_sig_tokens(&tokens, sig_start_idx);
                let cut_end = if matches!(kind.as_str(), "const" | "static" | "type") {
                    eq_pos.unwrap_or(j)
                } else {
                    j
                };
                let end_offset = if cut_end < n { tokens[cut_end].offset } else { content.len() };
                let sig_raw = if cut_end <= n && sig_start_idx < cut_end {
                    &content[tokens[sig_start_idx].offset..end_offset]
                } else {
                    ""
                };
                let signature = sanitize_signature(sig_raw);

                scan.symbols.push(Symbol {
                    name: name.clone(),
                    kind: kind.clone(),
                    line,
                    end_line: line,
                    public: pending_pub,
                    parent,
                    test: is_test,
                    signature,
                    decorators: Vec::new(),
                });
                pending_pub = false;
                if j < n {
                    if tokens[j].text == ";" {
                        scan.symbols[sym_idx].end_line = tokens[j].line;
                        i = j + 1;
                        continue;
                    } else if tokens[j].text == "{" {
                        let scope = if kind == "trait" {
                            Scope::Trait { name, is_test }
                        } else {
                            Scope::Other { is_test }
                        };
                        stack.push((scope, Some(sym_idx)));
                        i = j + 1;
                        continue;
                    }
                }
            }
        }

        // 6. Functions: `fn name(...)`
        if tok.text == "fn" && i + 1 < n && is_ident(&tokens[i + 1].text) {
            let fn_name = tokens[i + 1].text.clone();
            let line = tok.line;
            let is_test = pending_test_attr || stack.iter().any(|(s, _)| s.is_test());
            pending_test_attr = false;
            let parent = current_parent(&stack);

            let (j, _) = advance_sig_tokens(&tokens, i + 2);
            let end_offset = if j < n { tokens[j].offset } else { content.len() };
            let sig_raw = if j <= n && i + 2 < j {
                &content[tokens[i + 2].offset..end_offset]
            } else {
                ""
            };
            let signature = sanitize_signature(sig_raw);

            let sym_idx = scan.symbols.len();
            scan.symbols.push(Symbol {
                name: fn_name.clone(),
                kind: "fn".to_string(),
                line,
                end_line: line,
                public: pending_pub,
                parent,
                test: is_test,
                signature,
                decorators: Vec::new(),
            });
            pending_pub = false;
            if j < n {
                if tokens[j].text == ";" {
                    scan.symbols[sym_idx].end_line = tokens[j].line;
                    i = j + 1;
                    continue;
                } else if tokens[j].text == "{" {
                    stack.push((Scope::Fn { name: fn_name, is_test }, Some(sym_idx)));
                    i = j + 1;
                    continue;
                }
            }
        }

        // 7. `macro_rules! name`
        if tok.text == "macro_rules" && i + 1 < n && tokens[i + 1].text == "!" && i + 2 < n && is_ident(&tokens[i + 2].text) {
            let name = tokens[i + 2].text.clone();
            let line = tok.line;
            let is_test = stack.iter().any(|(s, _)| s.is_test());
            let sym_idx = scan.symbols.len();
            scan.symbols.push(Symbol {
                name,
                kind: "macro_rules".to_string(),
                line,
                end_line: line,
                public: pending_pub,
                parent: None,
                test: is_test,
                signature: String::new(),
                decorators: Vec::new(),
            });
            pending_pub = false;
            let mut j = i + 3;
            while j < n && tokens[j].text != "{" && tokens[j].text != ";" {
                j += 1;
            }
            if j < n {
                if tokens[j].text == ";" {
                    scan.symbols[sym_idx].end_line = tokens[j].line;
                    i = j + 1;
                    continue;
                } else if tokens[j].text == "{" {
                    stack.push((Scope::Other { is_test }, Some(sym_idx)));
                    i = j + 1;
                    continue;
                }
            }
        }

        // 8. `impl` headers: `impl[<...>] [Trait for] Type[<...>] {`
        if tok.text == "impl" {
            let is_test = stack.iter().any(|(s, _)| s.is_test());
            let mut j = i + 1;
            // Skip generics right after `impl<...>`
            if j < n && tokens[j].text == "<" {
                let mut depth = 1usize;
                j += 1;
                while j < n && depth > 0 {
                    if tokens[j].text == "<" {
                        depth += 1;
                    } else if tokens[j].text == ">" {
                        depth -= 1;
                    }
                    j += 1;
                }
            }
            // Collect tokens until `{`
            let mut header_tokens = Vec::new();
            while j < n && tokens[j].text != "{" && tokens[j].text != ";" {
                header_tokens.push(tokens[j].text.as_str());
                j += 1;
            }
            if j < n && tokens[j].text == "{" {
                let target = extract_impl_target(&header_tokens);
                stack.push((Scope::Impl { target, is_test }, None));
                pending_pub = false;
                pending_test_attr = false;
                i = j + 1;
                continue;
            }
        }

        // 9. Calls: `name(`, `Type::name(`, `.name(`
        if tok.text == "(" && i > 0 {
            let prev = &tokens[i - 1];
            if is_ident(&prev.text) && !is_keyword(&prev.text) && !is_excluded_call(&prev.text) {
                let is_macro = i >= 2 && tokens[i - 2].text == "!";
                if !is_macro {
                    let has_receiver = i >= 2 && tokens[i - 2].text == ".";
                    let qualifier = if !has_receiver && i >= 3 && tokens[i - 2].text == "::" && is_ident(&tokens[i - 3].text) {
                        Some(tokens[i - 3].text.clone())
                    } else {
                        None
                    };
                    let from = current_caller_from(&stack);
                    scan.calls.push(Call {
                        from,
                        name: prev.text.clone(),
                        line: prev.line,
                        has_receiver,
                        qualifier,
                    });
                }
            }
        }

        // 10. Braces tracking: { and }
        if tok.text == "{" {
            let is_test = stack.iter().any(|(s, _)| s.is_test());
            stack.push((Scope::Other { is_test }, None));
            pending_pub = false;
            pending_test_attr = false;
        } else if tok.text == "}" {
            if stack.len() > 1 {
                let (_, sym_idx_opt) = stack.pop().unwrap_or((Scope::Root, None));
                if let Some(idx) = sym_idx_opt {
                    if idx < scan.symbols.len() {
                        scan.symbols[idx].end_line = tok.line;
                    }
                }
            }
            pending_pub = false;
            pending_test_attr = false;
        } else if tok.text == ";" {
            pending_pub = false;
            pending_test_attr = false;
        }

        i += 1;
    }

    scan
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_alphabetic() => chars.all(|c| c == '_' || c.is_alphanumeric()),
        _ => false,
    }
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}

fn is_excluded_call(s: &str) -> bool {
    matches!(s, "Some" | "Ok" | "Err")
}

fn current_parent(stack: &[(Scope, Option<usize>)]) -> Option<String> {
    for (scope, _) in stack.iter().rev() {
        match scope {
            Scope::Impl { target, .. } => return Some(target.clone()),
            Scope::Trait { name, .. } => return Some(name.clone()),
            _ => {}
        }
    }
    None
}

fn current_caller_from(stack: &[(Scope, Option<usize>)]) -> String {
    let mut impl_type = None;
    let mut fn_name = None;

    for (scope, _) in stack.iter().rev() {
        match scope {
            Scope::Fn { name, .. } if fn_name.is_none() => {
                fn_name = Some(name.clone());
            }
            Scope::Impl { target, .. } if impl_type.is_none() => {
                impl_type = Some(target.clone());
            }
            Scope::Trait { name, .. } if impl_type.is_none() => {
                impl_type = Some(name.clone());
            }
            _ => {}
        }
    }

    match (impl_type, fn_name) {
        (Some(t), Some(f)) => format!("{t}::{f}"),
        (None, Some(f)) => f,
        _ => "-".to_string(),
    }
}

/// Extracts the target type from an `impl` header slice of tokens.
/// E.g. `["Trait", "for", "Type", "<", "T", ">"]` -> `"Type"`.
/// E.g. `["Type", "<", "T", ">"]` -> `"Type"`.
/// E.g. `["crate", "::", "sub", "::", "Collider"]` -> `"Collider"`.
fn extract_impl_target(tokens: &[&str]) -> String {
    let type_tokens = if let Some(pos) = tokens.iter().position(|&t| t == "for") {
        &tokens[pos + 1..]
    } else {
        tokens
    };

    // Find the primary type identifier, skipping `&`, lifetimes, `mut`, and generic brackets
    let mut candidate = String::new();
    let mut i = 0;
    while i < type_tokens.len() {
        let t = type_tokens[i];
        if t == "<" {
            // Nested generics: skip until matching >
            let mut depth = 1usize;
            i += 1;
            while i < type_tokens.len() && depth > 0 {
                if type_tokens[i] == "<" {
                    depth += 1;
                } else if type_tokens[i] == ">" {
                    depth -= 1;
                }
                i += 1;
            }
            continue;
        }
        if is_ident(t) && !matches!(t, "dyn" | "mut" | "const" | "unsafe") {
            candidate = t.to_string();
        }
        i += 1;
    }

    if candidate.is_empty() {
        "Anonymous".to_string()
    } else {
        candidate
    }
}

/// Expands a use expression containing braces `{a, b::{c, d}}` into individual use paths.
pub fn expand_use_expr(expr: &str) -> Vec<String> {
    expand_tree("", expr)
}

fn expand_tree(prefix: &str, input: &str) -> Vec<String> {
    let s = input.trim();
    if s.is_empty() {
        return Vec::new();
    }

    if let Some(brace_start) = s.find('{') {
        let before = s[..brace_start].trim_end_matches(':');
        let new_prefix = if prefix.is_empty() {
            before.to_string()
        } else if before.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}::{before}")
        };

        // Find matching closing brace
        let mut depth = 0usize;
        let mut brace_end = None;
        for (i, c) in s[brace_start..].char_indices() {
            if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    brace_end = Some(brace_start + i);
                    break;
                }
            }
        }

        if let Some(end) = brace_end {
            let inner = &s[brace_start + 1..end];
            let items = split_comma_respecting_braces(inner);
            let mut result = Vec::new();
            for it in items {
                let it = it.trim();
                if it.is_empty() {
                    continue;
                }
                if it == "self" {
                    result.push(new_prefix.clone());
                } else {
                    result.extend(expand_tree(&new_prefix, it));
                }
            }
            return result;
        }
    }

    // No braces
    let mut leaf = s.trim_matches(';');
    if let Some(pos) = leaf.find(" as ") {
        leaf = leaf[..pos].trim();
    }
    let path = if prefix.is_empty() {
        leaf.to_string()
    } else if leaf.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}::{leaf}")
    };
    vec![path]
}

fn split_comma_respecting_braces(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;

    for (i, c) in s.char_indices() {
        if c == '{' {
            depth += 1;
        } else if c == '}' && depth > 0 {
            depth -= 1;
        } else if c == ',' && depth == 0 {
            parts.push(&s[start..i]);
            start = i + 1;
        }
    }
    if start < s.len() {
        parts.push(&s[start..]);
    }
    parts
}


/// Resolves a single Rust import raw string against the known files in the repository.
pub fn resolve_import(
    current_file: &str,
    raw_import: &str,
    all_paths: &HashSet<String>,
    workspace_crates: &HashMap<String, String>,
) -> Option<String> {
    let normalized = raw_import.trim().trim_end_matches(';');

    // 1. `mod foo;`
    if let Some(mod_name) = normalized.strip_prefix("mod ") {
        let mod_name = mod_name.trim();
        let cur_dir = if current_file.ends_with("/lib.rs")
            || current_file.ends_with("/main.rs")
            || current_file.ends_with("/mod.rs")
            || !current_file.contains('/')
        {
            current_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
        } else {
            current_file.strip_suffix(".rs").unwrap_or(current_file)
        };

        let cand1 = if cur_dir.is_empty() {
            format!("{mod_name}.rs")
        } else {
            format!("{cur_dir}/{mod_name}.rs")
        };
        if all_paths.contains(&cand1) {
            return Some(cand1);
        }

        let cand2 = if cur_dir.is_empty() {
            format!("{mod_name}/mod.rs")
        } else {
            format!("{cur_dir}/{mod_name}/mod.rs")
        };
        if all_paths.contains(&cand2) {
            return Some(cand2);
        }
        return None;
    }

    // 2. `use crate::...`
    if let Some(rest) = normalized.strip_prefix("crate::") {
        let crate_src = find_crate_src(current_file);
        return resolve_path_in_dir(&crate_src, rest, all_paths);
    }

    // 3. `use super::...`
    if let Some(mut rest) = normalized.strip_prefix("super::") {
        let mut cur_dir = parent_module_dir(current_file);
        while let Some(r) = rest.strip_prefix("super::") {
            cur_dir = cur_dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            rest = r;
        }
        return resolve_path_in_dir(cur_dir, rest, all_paths);
    }

    // 4. `use <workspace_crate>::...`
    if let Some((first, rest)) = normalized.split_once("::") {
        if let Some(src_dir) = workspace_crates.get(first) {
            return resolve_path_in_dir(src_dir, rest, all_paths);
        }
    } else if let Some(src_dir) = workspace_crates.get(normalized) {
        let lib_path = format!("{src_dir}/lib.rs");
        if all_paths.contains(&lib_path) {
            return Some(lib_path);
        }
        let main_path = format!("{src_dir}/main.rs");
        if all_paths.contains(&main_path) {
            return Some(main_path);
        }
    }

    None
}

fn find_crate_src(current_file: &str) -> String {
    let mut parts: Vec<&str> = current_file.split('/').collect();
    if let Some(pos) = parts.iter().position(|&p| p == "src") {
        parts.truncate(pos + 1);
        return parts.join("/");
    }
    if let Some((parent, _)) = current_file.rsplit_once('/') {
        parent.to_string()
    } else {
        "".to_string()
    }
}

fn parent_module_dir(current_file: &str) -> &str {
    if current_file.ends_with("/mod.rs") {
        let folder = current_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        folder.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
    } else {
        current_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
    }
}

fn resolve_path_in_dir(base_dir: &str, sub_path: &str, all_paths: &HashSet<String>) -> Option<String> {
    let segs: Vec<&str> = sub_path
        .split("::")
        .filter(|s| !s.is_empty() && *s != "*")
        .collect();
    if segs.is_empty() {
        return None;
    }

    for k in (1..=segs.len()).rev() {
        let rel = segs[..k].join("/");
        let cand1 = if base_dir.is_empty() {
            format!("{rel}.rs")
        } else {
            format!("{base_dir}/{rel}.rs")
        };
        if all_paths.contains(&cand1) {
            return Some(cand1);
        }
        let cand2 = if base_dir.is_empty() {
            format!("{rel}/mod.rs")
        } else {
            format!("{base_dir}/{rel}/mod.rs")
        };
        if all_paths.contains(&cand2) {
            return Some(cand2);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fn_and_pub_fn() {
        let src = r#"
fn private_func() {
    let x = 1;
}

pub fn public_func() -> u32 {
    42
}

pub(crate) fn crate_func() {}
"#;
        let scan = scan_rust(src);
        assert_eq!(scan.symbols.len(), 3);
        assert_eq!(scan.symbols[0].name, "private_func");
        assert!(!scan.symbols[0].public);
        assert_eq!(scan.symbols[0].kind, "fn");

        assert_eq!(scan.symbols[1].name, "public_func");
        assert!(scan.symbols[1].public);

        assert_eq!(scan.symbols[2].name, "crate_func");
        assert!(scan.symbols[2].public);
    }

    #[test]
    fn impl_methods_with_parent_and_nested_generics() {
        let src = r#"
impl Collider {
    pub fn resolve_contacts(&self) {}
}

impl<T: Foo<Bar>> X for Y<T> {
    fn process(&mut self) {}
}
"#;
        let scan = scan_rust(src);
        let methods: Vec<_> = scan.symbols.into_iter().filter(|s| s.kind == "fn").collect();
        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].name, "resolve_contacts");
        assert_eq!(methods[0].parent.as_deref(), Some("Collider"));

        assert_eq!(methods[1].name, "process");
        assert_eq!(methods[1].parent.as_deref(), Some("Y"));
    }

    #[test]
    fn fn_and_braces_in_strings_and_comments_are_ignored() {
        let src = r##"
// fn fake_comment() {
/* fn fake_block() { */
let s = "fn in_string() {";
let raw = r#"fn in_raw() {"#;
let braw = br#"fn in_braw() {"#;

pub fn real_func() {}
"##;
        let scan = scan_rust(src);
        assert_eq!(scan.symbols.len(), 1);
        assert_eq!(scan.symbols[0].name, "real_func");
    }

    #[test]
    fn lifetime_vs_char_literal() {
        let src = r#"
fn with_lifetime<'a>(x: &'a str) -> char {
    let c = '{';
    let quote = '\'';
    let b = b'x';
    'z'
}
"#;
        let scan = scan_rust(src);
        assert_eq!(scan.symbols.len(), 1);
        assert_eq!(scan.symbols[0].name, "with_lifetime");
    }

    #[test]
    fn macro_rules_declaration() {
        let src = r#"
macro_rules! my_macro {
    () => {};
}
"#;
        let scan = scan_rust(src);
        assert_eq!(scan.symbols.len(), 1);
        assert_eq!(scan.symbols[0].name, "my_macro");
        assert_eq!(scan.symbols[0].kind, "macro_rules");
    }

    #[test]
    fn mod_tests_marked_as_test() {
        let src = r#"
pub fn normal() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_test() {
        assert!(true);
    }
}
"#;
        let scan = scan_rust(src);
        let normal = scan.symbols.iter().find(|s| s.name == "normal").unwrap();
        assert!(!normal.test);

        let unit_test = scan.symbols.iter().find(|s| s.name == "unit_test").unwrap();
        assert!(unit_test.test);
    }

    #[test]
    fn crlf_line_endings_match_editor() {
        let src = "fn first() {}\r\n\r\npub fn second() {}\r\n";
        let scan = scan_rust(src);
        assert_eq!(scan.symbols.len(), 2);
        assert_eq!(scan.symbols[0].name, "first");
        assert_eq!(scan.symbols[0].line, 1);

        assert_eq!(scan.symbols[1].name, "second");
        assert_eq!(scan.symbols[1].line, 3);
    }

    #[test]
    fn non_utf8_is_lossy_and_does_not_fail() {
        let bytes = b"fn test\xFF() {}\n";
        let clean = strip_comments_and_strings(bytes);
        assert!(clean.contains("fn test"));
    }

    #[test]
    fn unbalanced_braces_do_not_crash_scanner() {
        let src = r#"
fn broken() {
    let x = 1;
    // Missing closing brace at EOF
"#;
        let scan = scan_rust(src);
        assert_eq!(scan.symbols.len(), 1);
        assert_eq!(scan.symbols[0].name, "broken");
    }

    #[test]
    fn calls_exclude_keywords_and_macros_and_capture_caller() {
        let src = r#"
impl Calculator {
    fn compute(&self) {
        if true {
            self.add();
            saturate();
            println!("test");
            let x = Some(5);
        }
    }
}
"#;
        let scan = scan_rust(src);
        let calls: Vec<_> = scan.calls.into_iter().map(|c| (c.from, c.name)).collect();
        assert!(calls.contains(&("Calculator::compute".to_string(), "add".to_string())));
        assert!(calls.contains(&("Calculator::compute".to_string(), "saturate".to_string())));
        assert!(!calls.iter().any(|(_, name)| name == "if" || name == "println" || name == "Some"));
    }

    #[test]
    fn expand_nested_use_trees() {
        let expr = "crate::{model::Node, spatial::{depth, Target}}";
        let expanded = expand_use_expr(expr);
        assert_eq!(
            expanded,
            vec![
                "crate::model::Node".to_string(),
                "crate::spatial::depth".to_string(),
                "crate::spatial::Target".to_string()
            ]
        );
    }

    #[test]
    fn const_static_type_signatures_strip_assignment_and_value() {
        let src = r#"
pub const BUFFER_SIZE: usize = 1024;
pub const DIR: &str = ".cthg";
pub const MAGIC: &[u8; 4] = b"CTHG";
pub static mut TABLE: [i32; N] = [0; N];
pub const MAP: HashMap<String, Vec<u8>> = HashMap::new();
pub type Result<T> = std::result::Result<T, String>;
pub type Cache = HashMap<String, Vec<u8>>;
pub fn process(buf: &[u8; 4]) -> [i32; 8] {
    let x = 1;
}
"#;
        let scan = scan_rust(src);
        assert_eq!(scan.symbols.len(), 8);
        assert_eq!(scan.symbols[0].name, "BUFFER_SIZE");
        assert_eq!(scan.symbols[0].signature, ": usize");

        assert_eq!(scan.symbols[1].name, "DIR");
        assert_eq!(scan.symbols[1].signature, ": &str");

        assert_eq!(scan.symbols[2].name, "MAGIC");
        assert_eq!(scan.symbols[2].signature, ": &[u8; 4]");

        assert_eq!(scan.symbols[3].name, "TABLE");
        assert_eq!(scan.symbols[3].signature, ": [i32; N]");

        assert_eq!(scan.symbols[4].name, "MAP");
        assert_eq!(scan.symbols[4].signature, ": HashMap<String, Vec<u8>>");

        assert_eq!(scan.symbols[5].name, "Result");
        assert_eq!(scan.symbols[5].signature, "<T>");

        assert_eq!(scan.symbols[6].name, "Cache");
        assert_eq!(scan.symbols[6].signature, "");

        assert_eq!(scan.symbols[7].name, "process");
        assert_eq!(scan.symbols[7].signature, "(buf: &[u8; 4]) -> [i32; 8]");
    }
}
