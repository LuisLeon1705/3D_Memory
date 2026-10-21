//! HTML language scanner for the project index.
//!
//! Handles `.html` and `.htm` files.
//! Extracts script imports (`<script src="...">`), inline scripts, inline event
//! handlers (`onclick="..."` with `from` = `tag#id`), and elements with `id="..."`.

use crate::lang::{FileScan, Import, Lang, Symbol};
use std::collections::HashSet;

/// Strips HTML comments `<!-- ... -->` and template expressions (`{{ ... }}`, `{% ... %}`).
pub fn strip_html_comments_and_templates(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // 1. HTML comment <!-- ... -->
        if i + 3 < len && &bytes[i..i + 4] == b"<!--" {
            out.push(b' ');
            out.push(b' ');
            out.push(b' ');
            out.push(b' ');
            i += 4;
            while i < len {
                if i + 2 < len && &bytes[i..i + 3] == b"-->" {
                    out.push(b' ');
                    out.push(b' ');
                    out.push(b' ');
                    i += 3;
                    break;
                } else if bytes[i] == b'\n' {
                    out.push(b'\n');
                    i += 1;
                } else {
                    out.push(if bytes[i] == b'\r' { b'\r' } else { b' ' });
                    i += 1;
                }
            }
            continue;
        }

        // 2. Jinja / Django statement: {% ... %}
        if i + 1 < len && bytes[i] == b'{' && bytes[i + 1] == b'%' {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            while i < len {
                if i + 1 < len && bytes[i] == b'%' && bytes[i + 1] == b'}' {
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                    break;
                } else if bytes[i] == b'\n' {
                    out.push(b'\n');
                    i += 1;
                } else {
                    out.push(if bytes[i] == b'\r' { b'\r' } else { b' ' });
                    i += 1;
                }
            }
            continue;
        }

        // 3. Jinja / Django variable: {{ ... }}
        if i + 1 < len && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            while i < len {
                if i + 1 < len && bytes[i] == b'}' && bytes[i + 1] == b'}' {
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                    break;
                } else if bytes[i] == b'\n' {
                    out.push(b'\n');
                    i += 1;
                } else {
                    out.push(if bytes[i] == b'\r' { b'\r' } else { b' ' });
                    i += 1;
                }
            }
            continue;
        }

        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

pub fn scan_html_file(bytes: &[u8]) -> FileScan {
    let clean = strip_html_comments_and_templates(bytes);
    scan_html(&clean)
}

pub fn scan_html(source: &str) -> FileScan {
    let mut scan = FileScan::default();
    let bytes = source.as_bytes();
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

        if b == b'<' {
            // Check closing tag </...>
            if i + 1 < len && bytes[i + 1] == b'/' {
                i += 2;
                while i < len && bytes[i] != b'>' {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                continue;
            }

            // Tag start: read tag name
            let tag_start = i + 1;
            let mut j = tag_start;
            while j < len && !bytes[j].is_ascii_whitespace() && bytes[j] != b'>' && bytes[j] != b'/' {
                j += 1;
            }
            let tag_name = source[tag_start..j].to_ascii_lowercase();

            // Read tag attributes until '>'
            let mut attr_start = j;
            let mut in_quote: Option<u8> = None;
            while attr_start < len {
                let ab = bytes[attr_start];
                if ab == b'\n' {
                    line += 1;
                }
                if let Some(q) = in_quote {
                    if ab == q {
                        in_quote = None;
                    }
                } else if ab == b'\'' || ab == b'"' {
                    in_quote = Some(ab);
                } else if ab == b'>' {
                    break;
                }
                attr_start += 1;
            }

            let tag_content = if attr_start <= len && j < attr_start {
                &source[j..attr_start]
            } else {
                ""
            };

            let tag_line = line;

            // Extract element ID attribute if present
            let id_opt = extract_attribute_value(tag_content, "id");
            if let Some(ref id_val) = id_opt {
                if !id_val.is_empty() {
                    scan.symbols.push(Symbol {
                        name: id_val.clone(),
                        kind: "id".to_string(),
                        line: tag_line,
                        end_line: tag_line,
                        public: false,
                        parent: None,
                        test: false,
                        signature: String::new(),
                        decorators: Vec::new(),
                    });
                }
            }

            // Identify caller name: `tag#id` or `tag`
            let caller_name = if let Some(ref id_val) = id_opt {
                format!("{tag_name}#{id_val}")
            } else {
                tag_name.clone()
            };

            // If `<script src="...">` -> import
            if tag_name == "script" {
                if let Some(src) = extract_attribute_value(tag_content, "src") {
                    scan.imports.push(Import {
                        raw: format!("<script src=\"{src}\">"),
                        resolved: None,
                    });
                } else if attr_start < len && bytes[attr_start] == b'>' {
                    // Inline script: `<script>...</script>`
                    let script_start = attr_start + 1;
                    let mut script_end = script_start;
                    let script_start_line = line;

                    while script_end + 8 < len {
                        if bytes[script_end] == b'\n' {
                            line += 1;
                        }
                        if bytes[script_end] == b'<'
                            && bytes[script_end + 1] == b'/'
                            && source[script_end + 2..script_end + 8].eq_ignore_ascii_case("script")
                        {
                            break;
                        }
                        script_end += 1;
                    }

                    if script_end <= len && script_start < script_end {
                        let script_code = &source[script_start..script_end];
                        let js_scan = crate::lang_js::scan_js_file(Lang::Js, script_code.as_bytes());

                        for mut s in js_scan.symbols {
                            s.line += script_start_line - 1;
                            s.end_line += script_start_line - 1;
                            scan.symbols.push(s);
                        }
                        for mut c in js_scan.calls {
                            c.line += script_start_line - 1;
                            scan.calls.push(c);
                        }
                    }

                    i = script_end;
                    continue;
                }
            }

            // Extract inline event calls: `on*="..."` or `href="javascript:..."`
            for (attr, val) in extract_event_attributes(tag_content) {
                if attr.starts_with("on") || (attr == "href" && val.starts_with("javascript:")) {
                    let code = if let Some(rest) = val.strip_prefix("javascript:") {
                        rest
                    } else {
                        &val
                    };
                    let js_scan = crate::lang_js::scan_js_file(Lang::Js, code.as_bytes());
                    for mut c in js_scan.calls {
                        c.from = caller_name.clone();
                        c.line = tag_line;
                        scan.calls.push(c);
                    }
                }
            }

            i = attr_start + 1;
            continue;
        }

        i += 1;
    }

    scan
}

fn extract_attribute_value(tag_content: &str, attr_name: &str) -> Option<String> {
    for part in tag_content.split_whitespace() {
        if let Some((k, v)) = part.split_once('=') {
            if k.trim().eq_ignore_ascii_case(attr_name) {
                let trimmed = v.trim().trim_matches(['\'', '"']);
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn extract_event_attributes(tag_content: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let bytes = tag_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i].is_ascii_alphabetic() {
            let start_k = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_') {
                i += 1;
            }
            let key = tag_content[start_k..i].to_ascii_lowercase();

            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < len && bytes[i] == b'=' {
                i += 1;
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < len && (bytes[i] == b'\'' || bytes[i] == b'"') {
                    let quote = bytes[i];
                    i += 1;
                    let start_v = i;
                    while i < len && bytes[i] != quote {
                        i += 1;
                    }
                    let val = tag_content[start_v..i].to_string();
                    if i < len {
                        i += 1;
                    }
                    results.push((key, val));
                    continue;
                }
            }
        }
        i += 1;
    }

    results
}

/// Resolves HTML `<script src="...">` relative and absolute imports.
pub fn resolve_import(
    current_file: &str,
    raw_import: &str,
    all_paths: &HashSet<String>,
) -> Option<String> {
    let src = if let Some(pos) = raw_import.find("src=\"") {
        let rest = &raw_import[pos + 5..];
        rest.split('"').next()?
    } else if let Some(pos) = raw_import.find("src='") {
        let rest = &raw_import[pos + 5..];
        rest.split('\'').next()?
    } else {
        return None;
    };

    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("//") {
        return None; // External script
    }

    if src.starts_with('/') {
        // Absolute path from repo root or public/static
        let stripped = src.trim_start_matches('/');
        if all_paths.contains(stripped) {
            return Some(stripped.to_string());
        }
        let cand_public = format!("public/{stripped}");
        if all_paths.contains(&cand_public) {
            return Some(cand_public);
        }
        let cand_static = format!("static/{stripped}");
        if all_paths.contains(&cand_static) {
            return Some(cand_static);
        }
    } else {
        // Relative path from current HTML file's directory
        let norm_cur = current_file.replace('\\', "/");
        let cur_dir = norm_cur.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let combined = if cur_dir.is_empty() {
            src.to_string()
        } else {
            format!("{cur_dir}/{src}")
        };
        let normalized = normalize_rel_path(&combined);
        if all_paths.contains(&normalized) {
            return Some(normalized);
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
    fn test_html_script_imports_and_events() {
        let html = r#"
<!DOCTYPE html>
<html>
<head>
    <script src="js/app.js"></script>
    <script src="/bundle.js"></script>
    <script src="https://cdn.example.com/lib.js"></script>
</head>
<body>
    <button id="guardar" onclick="guardar(); notificar()">Guardar</button>
    <a href="javascript:cancelar()">Cancelar</a>
</body>
</html>
"#;
        let scan = scan_html(html);

        // Imports
        assert_eq!(scan.imports.len(), 3);

        // ID symbol
        let id_sym = scan.symbols.iter().find(|s| s.name == "guardar").unwrap();
        assert_eq!(id_sym.kind, "id");
        assert!(!id_sym.public);

        // Calls
        let call_names: Vec<_> = scan.calls.iter().map(|c| c.name.as_str()).collect();
        assert!(call_names.contains(&"guardar"));
        assert!(call_names.contains(&"notificar"));
        assert!(call_names.contains(&"cancelar"));

        let guardar_call = scan.calls.iter().find(|c| c.name == "guardar").unwrap();
        assert_eq!(guardar_call.from, "button#guardar");
    }

    #[test]
    fn test_html_inline_script() {
        let html = r#"
<html>
<body>
    <script>
        function inlineCalculate(x) {
            return x * 2;
        }
    </script>
</body>
</html>
"#;
        let scan = scan_html(html);
        let calc_sym = scan.symbols.iter().find(|s| s.name == "inlineCalculate").unwrap();
        assert_eq!(calc_sym.kind, "fn");
    }

    #[test]
    fn test_html_jinja_templates_ignored() {
        let html = r#"
<div class="user-info">
    {% if user.is_authenticated %}
        <span>{{ user.name }}</span>
        <button onclick="logout()">Salir</button>
    {% endif %}
</div>
"#;
        let scan = scan_html(html);
        assert!(scan.calls.iter().any(|c| c.name == "logout"));
    }

    #[test]
    fn test_html_import_resolution() {
        let mut paths = HashSet::new();
        paths.insert("index.html".to_string());
        paths.insert("js/app.js".to_string());
        paths.insert("public/bundle.js".to_string());

        let res1 = resolve_import("index.html", "<script src=\"js/app.js\">", &paths);
        assert_eq!(res1, Some("js/app.js".to_string()));

        let res2 = resolve_import("index.html", "<script src=\"/bundle.js\">", &paths);
        assert_eq!(res2, Some("public/bundle.js".to_string()));

        let res_cdn = resolve_import("index.html", "<script src=\"https://cdn.com/a.js\">", &paths);
        assert_eq!(res_cdn, None);
    }
}
