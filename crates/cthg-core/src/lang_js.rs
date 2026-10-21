//! JavaScript and TypeScript language scanner for the project index.
//!
//! Handles `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, and `.cts` files.
//! Extracts functions, classes, methods, arrow functions, constants, interfaces,
//! types, imports (ES modules, require, dynamic import), and calls (including JSX tags).

use crate::lang::{sanitize_signature, Call, FileScan, Import, Lang, Symbol};
use std::collections::HashSet;

/// Strips comments, strings, template literals (while preserving `${...}` expressions),
/// and regex literals from JS/TS source, preserving exact byte length and newlines.
pub fn strip_comments_and_strings(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let len = bytes.len();
    let mut i = 0;

    // Stack to track template literal nesting: inside template text vs inside ${...}
    // true = inside `${...}`, false = in normal code
    let mut template_stack: Vec<usize> = Vec::new(); // braces depth inside ${}

    // Previous non-whitespace token to distinguish regex from division
    let mut prev_token: Option<String> = None;

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

        // 2. Block comment /* ... */
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            while i < len {
                if bytes[i] == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
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
            continue;
        }

        // 3. String literals: '...' and "..."
        if b == b'\'' || b == b'"' {
            let quote = b;
            out.push(b);
            i += 1;
            while i < len {
                let ch = bytes[i];
                out.push(ch);
                i += 1;
                if (ch == quote && (i == 1 || bytes[i - 2] != b'\\')) || ch == b'\n' {
                    break;
                }
            }
            prev_token = Some("str".to_string());
            continue;
        }

        // 4. Template literals: `... ${expr} ...`
        if b == b'`' {
            out.push(b' ');
            i += 1;
            // Scan template string until unescaped ` or ${
            while i < len {
                if bytes[i] == b'`' && (i == 0 || bytes[i - 1] != b'\\') {
                    out.push(b' ');
                    i += 1;
                    break;
                } else if bytes[i] == b'$' && i + 1 < len && bytes[i + 1] == b'{' {
                    // Start of interpolation ${
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                    template_stack.push(1); // brace depth 1
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
            prev_token = Some("template".to_string());
            continue;
        }

        // Track braces if inside a template interpolation ${ ... }
        if !template_stack.is_empty() {
            if b == b'{' {
                if let Some(d) = template_stack.last_mut() {
                    *d += 1;
                }
            } else if b == b'}' {
                if let Some(d) = template_stack.last_mut() {
                    *d -= 1;
                    if *d == 0 {
                        template_stack.pop();
                        // Resume scanning template literal text
                        out.push(b' ');
                        i += 1;
                        while i < len {
                            if bytes[i] == b'`' && (i == 0 || bytes[i - 1] != b'\\') {
                                out.push(b' ');
                                i += 1;
                                break;
                            } else if bytes[i] == b'$' && i + 1 < len && bytes[i + 1] == b'{' {
                                out.push(b' ');
                                out.push(b' ');
                                i += 2;
                                template_stack.push(1);
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
                        prev_token = Some("template".to_string());
                        continue;
                    }
                }
            }
        }

        // 5. Regular expressions: /pattern/flags vs division /
        if b == b'/' {
            let is_regex = match prev_token.as_deref() {
                None => true,
                Some(tok) => matches!(
                    tok,
                    "(" | "," | "=" | ":" | "[" | "!" | "&" | "|" | "?" | "{" | "}" | ";" | "return" | "typeof" | "yield" | "case" | "throw" | "void" | "await"
                ),
            };

            if is_regex {
                out.push(b' ');
                i += 1;
                let mut in_char_class = false;
                while i < len {
                    if bytes[i] == b'[' && (i == 0 || bytes[i - 1] != b'\\') {
                        in_char_class = true;
                        out.push(b' ');
                        i += 1;
                    } else if bytes[i] == b']' && in_char_class && (i == 0 || bytes[i - 1] != b'\\') {
                        in_char_class = false;
                        out.push(b' ');
                        i += 1;
                    } else if bytes[i] == b'/' && !in_char_class && (i == 0 || bytes[i - 1] != b'\\') {
                        out.push(b' ');
                        i += 1;
                        // Skip regex flags (e.g. gimsuy)
                        while i < len && bytes[i].is_ascii_alphabetic() {
                            out.push(b' ');
                            i += 1;
                        }
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
                prev_token = Some("regex".to_string());
                continue;
            }
        }

        // Normal character
        if !b.is_ascii_whitespace() {
            let ch_str = (b as char).to_string();
            prev_token = Some(ch_str);
        }
        out.push(b);
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}


fn advance_js_sig_tokens(tokens: &[Token], start: usize) -> (usize, Option<usize>) {
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
                if j > start && (tokens[j - 1].text == "=" || tokens[j - 1].text == "-") {
                    // Part of `=>` or `->`
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

pub fn scan_js_file(lang: Lang, bytes: &[u8]) -> FileScan {
    let clean = strip_comments_and_strings(bytes);
    scan_js(lang, &clean)
}

#[derive(Clone, Debug)]
struct Token {
    text: String,
    line: usize,
    offset: usize,
}

fn tokenize_js(s: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut line = 1usize;

    while i < len {
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        let start = i;

        // String literals: '...' and "..."
        if b == b'\'' || b == b'"' {
            let quote = b;
            i += 1;
            while i < len {
                let ch = bytes[i];
                i += 1;
                if ch == quote && (i == 1 || bytes[i - 2] != b'\\') {
                    break;
                } else if ch == b'\n' {
                    line += 1;
                    break;
                }
            }
            tokens.push(Token {
                text: s[start..i].to_string(),
                line,
                offset: start,
            });
            continue;
        }

        // Identifiers and keywords (including #private fields)
        if b.is_ascii_alphabetic() || b == b'_' || b == b'$' || b == b'#' {
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') {
                i += 1;
            }
            tokens.push(Token {
                text: s[start..i].to_string(),
                line,
                offset: start,
            });
            continue;
        }

        // Multi-char operators: =>, ===, !==, ==, !=, <=, >=, &&, ||, ??, ?.
        if i + 2 < len {
            let three = &bytes[start..start + 3];
            let three_str = match three {
                b"===" => Some("==="),
                b"!==" => Some("!=="),
                b"..." => Some("..."),
                b">>>" => Some(">>>"),
                _ => None,
            };
            if let Some(op) = three_str {
                tokens.push(Token {
                    text: op.to_string(),
                    line,
                    offset: start,
                });
                i += 3;
                continue;
            }
        }
        if i + 1 < len {
            let two = &bytes[start..start + 2];
            let two_str = match two {
                b"=>" => Some("=>"),
                b"==" => Some("=="),
                b"!=" => Some("!="),
                b"<=" => Some("<="),
                b">=" => Some(">="),
                b"&&" => Some("&&"),
                b"||" => Some("||"),
                b"??" => Some("??"),
                b"?." => Some("?."),
                b"+=" => Some("+="),
                b"-=" => Some("-="),
                b"*=" => Some("*="),
                b"/=" => Some("/="),
                _ => None,
            };
            if let Some(op) = two_str {
                tokens.push(Token {
                    text: op.to_string(),
                    line,
                    offset: start,
                });
                i += 2;
                continue;
            }
        }

        // Single punctuation / unicode char
        if b.is_ascii() {
            tokens.push(Token {
                text: (b as char).to_string(),
                line,
                offset: start,
            });
            i += 1;
        } else {
            if let Some(ch) = s[start..].chars().next() {
                tokens.push(Token {
                    text: ch.to_string(),
                    line,
                    offset: start,
                });
                i += ch.len_utf8();
            } else {
                i += 1;
            }
        }
    }

    tokens
}

#[derive(Clone, Debug)]
enum Scope {
    Class { name: String },
    Fn,
    Other,
}

pub fn scan_js(_lang: Lang, source: &str) -> FileScan {
    let mut scan = FileScan::default();
    let tokens = tokenize_js(source);
    let n = tokens.len();

    let mut scope_stack: Vec<(Scope, Option<usize>)> = Vec::new();
    let mut pending_export = false;

    let mut i = 0;
    while i < n {
        let tok = &tokens[i];

        // Track braces
        if tok.text == "{" {
            scope_stack.push((Scope::Other, None));
            i += 1;
            continue;
        } else if tok.text == "}" {
            if let Some((_, Some(sym_idx))) = scope_stack.pop() {
                if scan.symbols[sym_idx].end_line == scan.symbols[sym_idx].line {
                    scan.symbols[sym_idx].end_line = tok.line;
                }
            }
            i += 1;
            continue;
        }

        // `export` or `export default`
        if tok.text == "export" {
            pending_export = true;
            if i + 1 < n && tokens[i + 1].text == "default" {
                i += 1;
            }
            i += 1;
            continue;
        }

        // 1. `import ... from '...'` or `import '...'`
        if tok.text == "import" {
            let line = tok.line;
            let mut j = i + 1;
            // Check dynamic import: import('...')
            if j < n && tokens[j].text == "(" {
                // Dynamic import
                if j + 1 < n {
                    let target = tokens[j + 1].text.trim_matches(['\'', '"']);
                    scan.imports.push(Import {
                        raw: format!("import('{target}')"),
                        resolved: None,
                    });
                }
            } else {
                // Static import
                while j < n && tokens[j].text != ";" && tokens[j].line == line {
                    if tokens[j].text == "from" && j + 1 < n {
                        let target = tokens[j + 1].text.trim_matches(['\'', '"']);
                        scan.imports.push(Import {
                            raw: format!("import from '{target}'"),
                            resolved: None,
                        });
                        break;
                    }
                    j += 1;
                }
            }
            i += 1;
            continue;
        }

        // 2. `require('...')`
        if tok.text == "require" && i + 2 < n && tokens[i + 1].text == "(" {
            let target = tokens[i + 2].text.trim_matches(['\'', '"']);
            scan.imports.push(Import {
                raw: format!("require('{target}')"),
                resolved: None,
            });
            i += 3;
            continue;
        }

        // 3. Classes: `class Name extends Base implements Iface {`
        if tok.text == "class" && i + 1 < n && is_ident(&tokens[i + 1].text) {
            let class_name = tokens[i + 1].text.clone();
            let line = tok.line;
            let sym_idx = scan.symbols.len();

            let mut j = i + 2;
            while j < n && tokens[j].text != "{" {
                j += 1;
            }

            let sig_raw = if j > i + 2 && j <= n {
                &source[tokens[i + 2].offset..tokens[j].offset]
            } else {
                ""
            };

            scan.symbols.push(Symbol {
                name: class_name.clone(),
                kind: "class".to_string(),
                line,
                end_line: line,
                public: pending_export,
                parent: None,
                test: false,
                signature: sanitize_signature(sig_raw),
                decorators: Vec::new(),
            });
            pending_export = false;

            if j < n && tokens[j].text == "{" {
                scope_stack.push((Scope::Class { name: class_name }, Some(sym_idx)));
                i = j + 1;
                continue;
            }
        }

        // 4. Methods inside class scope: `method(...) {`, `async method(...) {`, `get / set`
        let parent_class = match scope_stack.last() {
            Some((Scope::Class { name }, _)) => Some(name.clone()),
            _ => None,
        };

        if let Some(ref class_name) = parent_class {
            let mut k = i;
            if tok.text == "async" || tok.text == "static" || tok.text == "get" || tok.text == "set" {
                k += 1;
            }
            if k < n && is_ident(&tokens[k].text) && k + 1 < n && tokens[k + 1].text == "(" {
                let method_name = tokens[k].text.clone();
                let line = tokens[k].line;
                let sym_idx = scan.symbols.len();

                let mut j = k + 1;
                let mut p_depth = 0usize;
                while j < n {
                    if tokens[j].text == "(" {
                        p_depth += 1;
                    } else if tokens[j].text == ")" {
                        p_depth -= 1;
                        if p_depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    j += 1;
                }
                while j < n && tokens[j].text != "{" && tokens[j].text != ";" {
                    j += 1;
                }

                let sig_raw = if j <= n && k + 1 < j {
                    &source[tokens[k + 1].offset..tokens[j].offset]
                } else {
                    ""
                };

                let is_pub = !method_name.starts_with('#') && !method_name.starts_with('_');

                scan.symbols.push(Symbol {
                    name: method_name,
                    kind: "fn".to_string(),
                    line,
                    end_line: line,
                    public: is_pub,
                    parent: Some(class_name.clone()),
                    test: false,
                    signature: sanitize_signature(sig_raw),
                    decorators: Vec::new(),
                });

                if j < n && tokens[j].text == "{" {
                    scope_stack.push((Scope::Fn, Some(sym_idx)));
                    i = j + 1;
                    continue;
                }
            }
        }

        // 5. Functions: `function name(...)`, `async function name(...)`, `function*`
        let mut is_fn = tok.text == "function";
        let mut fn_pos = i;
        if tok.text == "async" && i + 1 < n && tokens[i + 1].text == "function" {
            is_fn = true;
            fn_pos = i + 1;
        }

        if is_fn {
            let mut name_pos = fn_pos + 1;
            if name_pos < n && tokens[name_pos].text == "*" {
                name_pos += 1;
            }
            if name_pos < n && is_ident(&tokens[name_pos].text) {
                let fn_name = tokens[name_pos].text.clone();
                let line = tok.line;
                let sym_idx = scan.symbols.len();

                let mut j = name_pos + 1;
                while j < n && tokens[j].text != "{" && tokens[j].text != ";" {
                    j += 1;
                }

                let sig_raw = if j <= n && name_pos + 1 < j {
                    &source[tokens[name_pos + 1].offset..tokens[j].offset]
                } else {
                    ""
                };

                scan.symbols.push(Symbol {
                    name: fn_name,
                    kind: "fn".to_string(),
                    line,
                    end_line: line,
                    public: pending_export,
                    parent: None,
                    test: false,
                    signature: sanitize_signature(sig_raw),
                    decorators: Vec::new(),
                });
                pending_export = false;

                if j < n && tokens[j].text == "{" {
                    scope_stack.push((Scope::Fn, Some(sym_idx)));
                    i = j + 1;
                    continue;
                }
            }
        }

        // 6. Arrow functions or const assignments: `const f = (...) =>`
        if matches!(tok.text.as_str(), "const" | "let" | "var") && i + 1 < n && is_ident(&tokens[i + 1].text) {
            let var_name = tokens[i + 1].text.clone();
            let line = tok.line;

            // Check if arrow function: look for `=` and then `=>`
            let mut is_arrow = false;
            let mut arrow_pos = None;
            let mut j = i + 2;
            while j < n && j < i + 30 && tokens[j].text != ";" && tokens[j].text != "{" {
                if tokens[j].text == "=>" {
                    is_arrow = true;
                    arrow_pos = Some(j);
                    break;
                }
                j += 1;
            }

            if is_arrow {
                let sym_idx = scan.symbols.len();
                let sig_raw = if let Some(a_pos) = arrow_pos {
                    if a_pos < n && i + 2 < a_pos {
                        let raw = &source[tokens[i + 2].offset..tokens[a_pos + 1].offset];
                        raw.trim_start_matches('=').trim()
                    } else {
                        ""
                    }
                } else {
                    ""
                };

                scan.symbols.push(Symbol {
                    name: var_name,
                    kind: "fn".to_string(),
                    line,
                    end_line: line,
                    public: pending_export,
                    parent: None,
                    test: false,
                    signature: sanitize_signature(sig_raw),
                    decorators: Vec::new(),
                });
                pending_export = false;

                // Advance
                let mut k = arrow_pos.unwrap() + 1;
                while k < n && tokens[k].text != "{" && tokens[k].text != ";" {
                    k += 1;
                }
                if k < n && tokens[k].text == "{" {
                    scope_stack.push((Scope::Fn, Some(sym_idx)));
                    i = k + 1;
                    continue;
                }
            } else if tok.text == "const" && scope_stack.is_empty() {
                // Top-level module constant
                let is_const = var_name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
                if is_const {
                    let mut type_sig = String::new();
                    if i + 2 < n && tokens[i + 2].text == ":" {
                        let (j, eq_pos) = advance_js_sig_tokens(&tokens, i + 2);
                        let cut_end = eq_pos.unwrap_or(j);
                        if cut_end <= n && i + 2 < cut_end {
                            let end_offset = if cut_end < n { tokens[cut_end].offset } else { source.len() };
                            let raw = &source[tokens[i + 2].offset..end_offset];
                            type_sig = sanitize_signature(raw);
                        }
                    }
                    scan.symbols.push(Symbol {
                        name: var_name,
                        kind: "const".to_string(),
                        line,
                        end_line: line,
                        public: pending_export,
                        parent: None,
                        test: false,
                        signature: type_sig,
                        decorators: Vec::new(),
                    });
                    pending_export = false;
                }
            }
        }

        // 7. TypeScript interfaces, types, enums
        if matches!(tok.text.as_str(), "interface" | "type" | "enum") && i + 1 < n && is_ident(&tokens[i + 1].text) {
            let kind = tok.text.clone();
            let name = tokens[i + 1].text.clone();
            let line = tok.line;

            let (j, eq_pos) = advance_js_sig_tokens(&tokens, i + 2);
            let cut_end = if kind == "type" {
                eq_pos.unwrap_or(j)
            } else {
                j
            };

            let end_offset = if cut_end < n { tokens[cut_end].offset } else { source.len() };
            let sig_raw = if cut_end <= n && i + 2 < cut_end {
                &source[tokens[i + 2].offset..end_offset]
            } else {
                ""
            };

            scan.symbols.push(Symbol {
                name,
                kind,
                line,
                end_line: line,
                public: pending_export,
                parent: None,
                test: false,
                signature: sanitize_signature(sig_raw),
                decorators: Vec::new(),
            });
            pending_export = false;
        }

        // 8. Calls: `name(`, `obj.name(`, `new Name(`, or JSX `<Component`
        if is_ident(&tok.text) && i + 1 < n && tokens[i + 1].text == "(" && !is_js_keyword(&tok.text) {
            let caller_from = current_caller_from(&scope_stack, &scan.symbols);
            scan.calls.push(Call {
                from: caller_from,
                name: tok.text.clone(),
                line: tok.line,
            });
        }

        // JSX tag: `<Component` (starting with uppercase)
        if tok.text == "<" && i + 1 < n && is_jsx_component(&tokens[i + 1].text) {
            let caller_from = current_caller_from(&scope_stack, &scan.symbols);
            scan.calls.push(Call {
                from: caller_from,
                name: tokens[i + 1].text.clone(),
                line: tok.line,
            });
        }

        i += 1;
    }

    scan
}

fn current_caller_from(scope_stack: &[(Scope, Option<usize>)], symbols: &[Symbol]) -> String {
    scope_stack
        .iter()
        .rev()
        .find_map(|(_, sym_idx_opt)| {
            let sym_idx = (*sym_idx_opt)?;
            let sym = &symbols[sym_idx];
            if let Some(ref parent) = sym.parent {
                Some(format!("{}::{}", parent, sym.name))
            } else {
                Some(sym.name.clone())
            }
        })
        .unwrap_or_else(|| "(nivel de módulo)".to_string())
}

fn is_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' && first != '#' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn is_jsx_component(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn is_js_keyword(s: &str) -> bool {
    matches!(
        s,
        "if" | "for" | "while" | "switch" | "catch" | "with" | "return" | "typeof"
            | "instanceof" | "yield" | "await" | "function" | "class" | "import"
            | "export" | "default" | "extends" | "implements" | "new" | "delete"
    )
}

/// Resolves a JS/TS relative import against known repository files.
pub fn resolve_import(
    current_file: &str,
    raw_import: &str,
    all_paths: &HashSet<String>,
) -> Option<String> {
    let raw = raw_import.trim();
    let target = if let Some(pos) = raw.find('\'') {
        let rest = &raw[pos + 1..];
        rest.split('\'').next().unwrap_or("").trim()
    } else if let Some(pos) = raw.find('"') {
        let rest = &raw[pos + 1..];
        rest.split('"').next().unwrap_or("").trim()
    } else {
        return None;
    };

    if !target.starts_with("./") && !target.starts_with("../") {
        return None; // Bare imports (npm) remain unresolved
    }

    let norm_cur = current_file.replace('\\', "/");
    let cur_dir = norm_cur.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let combined = if cur_dir.is_empty() {
        target.to_string()
    } else {
        format!("{cur_dir}/{target}")
    };

    let normalized = normalize_rel_path(&combined);

    // 1. Exact path
    if all_paths.contains(&normalized) {
        return Some(normalized);
    }

    // 2. Probing extensions: .ts, .tsx, .js, .jsx, .mjs, .cjs
    const EXTENSIONS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];
    for ext in EXTENSIONS {
        let cand = format!("{normalized}{ext}");
        if all_paths.contains(&cand) {
            return Some(cand);
        }
    }

    // 3. Probing index: /index.ts, /index.tsx, /index.js, /index.jsx
    const INDEX_EXTS: &[&str] = &["/index.ts", "/index.tsx", "/index.js", "/index.jsx"];
    for idx_ext in INDEX_EXTS {
        let cand = format!("{normalized}{idx_ext}");
        if all_paths.contains(&cand) {
            return Some(cand);
        }
    }

    None
}

fn normalize_rel_path(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_functions_and_classes() {
        let js = r#"
export class ApiClient extends BaseClient {
    async fetchUser(id: string): Promise<User> {
        return this.get(id);
    }
}

export const helper = (a: number, b = 1): number => {
    return a + b;
};

function privateUtility() {
    console.log("util");
}
"#;
        let scan = scan_js(Lang::Ts, js);
        assert_eq!(scan.symbols.len(), 4);

        // ApiClient
        assert_eq!(scan.symbols[0].name, "ApiClient");
        assert_eq!(scan.symbols[0].kind, "class");
        assert!(scan.symbols[0].public);
        assert_eq!(scan.symbols[0].signature, "extends BaseClient");

        // fetchUser
        assert_eq!(scan.symbols[1].name, "fetchUser");
        assert_eq!(scan.symbols[1].kind, "fn");
        assert_eq!(scan.symbols[1].parent.as_deref(), Some("ApiClient"));
        assert_eq!(scan.symbols[1].signature, "(id: string): Promise<User>");

        // helper
        assert_eq!(scan.symbols[2].name, "helper");
        assert_eq!(scan.symbols[2].kind, "fn");
        assert!(scan.symbols[2].public);

        // privateUtility
        assert_eq!(scan.symbols[3].name, "privateUtility");
        assert!(!scan.symbols[3].public);
    }

    #[test]
    fn test_js_regex_vs_division() {
        let js = r#"
const re = /pattern\/{foo}\/\//g;
const div = a / b;
const div2 = (x + y) / 2;
"#;
        let clean = strip_comments_and_strings(js.as_bytes());
        // Clean should not contain pattern, but div and div2 should remain
        assert!(!clean.contains("pattern"));
        assert!(clean.contains('/'));
    }

    #[test]
    fn test_js_nested_template_literals() {
        let js = r#"
const msg = `hello ${name + ` inner ${calc()}`} world`;
"#;
        let clean = strip_comments_and_strings(js.as_bytes());
        assert!(!clean.contains("hello"));
        assert!(!clean.contains("world"));
        assert!(clean.contains("calc()"));
    }

    #[test]
    fn test_tsx_generic_vs_jsx_call() {
        let tsx = r#"
function genericFunc<T>(arg: T): T {
    return <UserCard user={arg} />;
}
"#;
        let scan = scan_js(Lang::Ts, tsx);
        let calls: Vec<_> = scan.calls.iter().map(|c| c.name.as_str()).collect();
        assert!(calls.contains(&"UserCard"));
        assert!(!calls.contains(&"T"));
    }

    #[test]
    fn test_js_import_resolution() {
        let mut paths = HashSet::new();
        paths.insert("src/api.ts".to_string());
        paths.insert("src/components/Card/index.tsx".to_string());
        paths.insert("src/utils.js".to_string());

        let res1 = resolve_import("src/app.ts", "import { get } from './api'", &paths);
        assert_eq!(res1, Some("src/api.ts".to_string()));

        let res2 = resolve_import("src/app.ts", "import Card from './components/Card'", &paths);
        assert_eq!(res2, Some("src/components/Card/index.tsx".to_string()));

        let res_ext = resolve_import("src/app.ts", "import React from 'react'", &paths);
        assert_eq!(res_ext, None);
    }

    #[test]
    fn test_js_unicode_characters_and_operators() {
        let js = r#"
// Comparison with approximate symbol:
const approx = a ≈ b;
function calcular(valor) {
    return valor * 2;
}
"#;
        let scan = scan_js_file(Lang::Js, js.as_bytes());

        assert_eq!(scan.symbols.len(), 1);
        assert_eq!(scan.symbols[0].name, "calcular");
    }

    #[test]
    fn test_js_call_in_anon_fn_and_iife_attribution() {
        let js = r#"
// Top-level / IIFE call
(function() {
    initApp();
})();

// Call inside anonymous arrow in named function
function doWork() {
    setTimeout(() => {
        helper();
    }, 100);
}

// Call inside anonymous function in class method
class StorageManager {
    save() {
        items.forEach(function(item) {
            persist(item);
        });
    }
}
"#;
        let scan = scan_js_file(Lang::Js, js.as_bytes());

        let find_call = |name: &str| scan.calls.iter().find(|c| c.name == name).unwrap().clone();

        assert_eq!(find_call("initApp").from, "(nivel de módulo)");
        assert_eq!(find_call("helper").from, "doWork");
        assert_eq!(find_call("persist").from, "StorageManager::save");
    }

    #[test]
    fn js_const_and_type_strip_assignment_and_value() {
        let ts = r#"
export const API_TIMEOUT: number = 5000;
export const DEFAULT_HOST = "localhost";
export const BUF: [number, number] = [1, 2];
export const MAP: Map<string, Array<number>> = new Map();
export type UserId<T> = string | T;
export type Pair = [number, number];
"#;
        let scan = scan_js_file(Lang::Ts, ts.as_bytes());
        let const_sym = scan.symbols.iter().find(|s| s.name == "API_TIMEOUT").unwrap();
        assert_eq!(const_sym.signature, ": number");

        let host_sym = scan.symbols.iter().find(|s| s.name == "DEFAULT_HOST").unwrap();
        assert_eq!(host_sym.signature, "");

        let buf_sym = scan.symbols.iter().find(|s| s.name == "BUF").unwrap();
        assert_eq!(buf_sym.signature, ": [number, number]");

        let map_sym = scan.symbols.iter().find(|s| s.name == "MAP").unwrap();
        assert_eq!(map_sym.signature, ": Map<string, Array<number>>");

        let type_sym = scan.symbols.iter().find(|s| s.name == "UserId").unwrap();
        assert_eq!(type_sym.signature, "<T>");

        let pair_sym = scan.symbols.iter().find(|s| s.name == "Pair").unwrap();
        assert_eq!(pair_sym.signature, "");
    }
}
