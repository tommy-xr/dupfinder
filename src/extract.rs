//! Multi-language function/type extraction: Rust and TypeScript via
//! tree-sitter, Functor Lang (.fun) via a line-based parser (top-level
//! bindings only, which is Functor's whole reuse surface: file = module).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use tree_sitter::{Node, Parser};

#[derive(Clone, Serialize, Deserialize)]
pub struct FnRecord {
    pub name: String,
    /// Enclosing impl/trait/mod/class chain, outermost first.
    pub context: String,
    pub sig: String,
    pub doc: String,
    pub body: String,
    /// Root-relative path, '/'-separated.
    pub file: String,
    /// 1-based lines.
    pub start: u32,
    pub end: u32,
    pub lines: u32,
    pub lang: String,
    pub public: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TypeRecord {
    pub name: String,
    pub kind: String,
    pub doc: String,
    pub file: String,
    pub start: u32,
    pub lang: String,
    pub public: bool,
}

pub struct Extraction {
    pub fns: Vec<FnRecord>,
    pub types: Vec<TypeRecord>,
    /// fnv1a content hash per extracted file (for incremental re-embedding).
    pub file_hashes: BTreeMap<String, String>,
}

impl FnRecord {
    pub fn is_testish(&self) -> bool {
        self.context.contains("mod tests")
            || self.file.contains("/tests/")
            || self.file.contains("/test/")
            || self.file.contains("/__tests__/")
            || self.file.ends_with(".test.ts")
            || self.file.ends_with(".spec.ts")
            || self.file.ends_with(".test.js")
            || self.file.ends_with(".spec.js")
    }

    /// The text that gets embedded: context + signature + doc + truncated body.
    pub fn embed_text(&self) -> String {
        let body = truncate_chars(&self.body, 2000);
        format!("{} {}\n{}\n{}", self.context, self.sig, self.doc, body)
    }

    pub fn location(&self) -> String {
        format!("{}:{}", self.file, self.start)
    }
}

fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

pub fn fnv1a(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

const SKIP_DIRS: &[&str] = &["node_modules", "target", "dist", "build", ".git", "vendor"];

pub fn extract_dir(root: &Path) -> Result<Extraction> {
    let mut files: Vec<std::path::PathBuf> = ignore::WalkBuilder::new(root)
        .hidden(true)
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .filter(|p| {
            !p.components().any(|c| {
                c.as_os_str()
                    .to_str()
                    .is_some_and(|s| SKIP_DIRS.contains(&s))
            })
        })
        .collect();
    files.sort();

    let mut ex = Extraction {
        fns: Vec::new(),
        types: Vec::new(),
        file_hashes: BTreeMap::new(),
    };

    let mut rust_parser = Parser::new();
    rust_parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .context("load rust grammar")?;
    let mut ts_parser = Parser::new();
    ts_parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .context("load ts grammar")?;
    let mut tsx_parser = Parser::new();
    tsx_parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .context("load tsx grammar")?;

    for path in files {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".d.ts") {
            continue;
        }
        let parser: Option<&mut Parser> = match ext {
            "rs" => Some(&mut rust_parser),
            "ts" | "mts" | "cts" => Some(&mut ts_parser),
            "tsx" | "jsx" | "js" | "mjs" | "cjs" => Some(&mut tsx_parser),
            "fun" => None,
            _ => continue,
        };
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        ex.file_hashes.insert(rel.clone(), fnv1a(src.as_bytes()));

        match parser {
            Some(p) => {
                let lang = if ext == "rs" { "rust" } else { "typescript" };
                let Some(tree) = p.parse(&src, None) else {
                    continue;
                };
                if lang == "rust" {
                    walk_rust(tree.root_node(), &src, &rel, &mut ex);
                } else {
                    walk_ts(tree.root_node(), &src, &rel, &mut ex);
                }
            }
            None => extract_functor(&src, &rel, &mut ex),
        }
    }
    Ok(ex)
}

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------- Rust

fn rust_doc_before(node: Node, src: &str) -> String {
    let mut parts = Vec::new();
    let mut sib = node.prev_named_sibling();
    while let Some(s) = sib {
        match s.kind() {
            "line_comment" => {
                let t = text(s, src);
                if let Some(stripped) = t.strip_prefix("///") {
                    parts.push(stripped.trim().to_string());
                } else {
                    break;
                }
            }
            // Keep walking past #[derive]/#[cfg]/#[test] to the docs above them.
            "attribute_item" => {}
            _ => break,
        }
        sib = s.prev_named_sibling();
    }
    parts.reverse();
    let mut doc = parts.join(" ");
    doc.truncate(doc.char_indices().nth(300).map_or(doc.len(), |(i, _)| i));
    doc
}

fn rust_context(node: Node, src: &str) -> String {
    let mut parts = Vec::new();
    let mut cur = node.parent();
    while let Some(n) = cur {
        match n.kind() {
            "impl_item" => {
                let ty = n
                    .child_by_field_name("type")
                    .map(|t| collapse_ws(text(t, src)))
                    .unwrap_or_else(|| "?".into());
                match n.child_by_field_name("trait") {
                    Some(tr) => parts.push(format!("impl {} for {}", collapse_ws(text(tr, src)), ty)),
                    None => parts.push(format!("impl {ty}")),
                }
            }
            "trait_item" => {
                if let Some(name) = n.child_by_field_name("name") {
                    parts.push(format!("trait {}", text(name, src)));
                }
            }
            "mod_item" => {
                if let Some(name) = n.child_by_field_name("name") {
                    parts.push(format!("mod {}", text(name, src)));
                }
            }
            _ => {}
        }
        cur = n.parent();
    }
    parts.reverse();
    parts.join(" / ")
}

fn walk_rust(node: Node, src: &str, file: &str, ex: &mut Extraction) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let body = text(child, src);
                    let sig_end = child
                        .child_by_field_name("body")
                        .map(|b| b.start_byte() - child.start_byte())
                        .unwrap_or(body.len());
                    let sig = collapse_ws(&body[..sig_end]);
                    let start = child.start_position().row as u32 + 1;
                    let end = child.end_position().row as u32 + 1;
                    ex.fns.push(FnRecord {
                        name: text(name_node, src).to_string(),
                        context: rust_context(child, src),
                        doc: rust_doc_before(child, src),
                        public: sig.starts_with("pub"),
                        sig,
                        body: body.to_string(),
                        file: file.to_string(),
                        start,
                        end,
                        lines: end - start + 1,
                        lang: "rust".into(),
                    });
                }
            }
            "struct_item" | "enum_item" | "trait_item" | "type_item" | "union_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let head = text(child, src);
                    ex.types.push(TypeRecord {
                        name: text(name_node, src).to_string(),
                        kind: child.kind().trim_end_matches("_item").to_string(),
                        doc: rust_doc_before(child, src),
                        file: file.to_string(),
                        start: child.start_position().row as u32 + 1,
                        lang: "rust".into(),
                        public: head.starts_with("pub"),
                    });
                }
            }
            _ => {}
        }
        walk_rust(child, src, file, ex);
    }
}

// ---------------------------------------------------------- TypeScript

fn ts_doc_before(node: Node, src: &str) -> String {
    let mut sib = node.prev_named_sibling();
    // Hop over `export` wrappers handled by callers passing the outermost node.
    while let Some(s) = sib {
        if s.kind() == "comment" {
            let t = text(s, src);
            if t.starts_with("/**") || t.starts_with("//") {
                let cleaned: String = t
                    .lines()
                    .map(|l| {
                        l.trim()
                            .trim_start_matches("/**")
                            .trim_start_matches("*/")
                            .trim_start_matches("//")
                            .trim_start_matches('*')
                            .trim()
                    })
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut doc = cleaned;
                doc.truncate(doc.char_indices().nth(300).map_or(doc.len(), |(i, _)| i));
                return doc;
            }
            return String::new();
        }
        if s.kind() == "decorator" {
            sib = s.prev_named_sibling();
            continue;
        }
        return String::new();
    }
    String::new()
}

fn ts_context(node: Node, src: &str) -> String {
    let mut parts = Vec::new();
    let mut cur = node.parent();
    while let Some(n) = cur {
        match n.kind() {
            "class_declaration" | "abstract_class_declaration" => {
                if let Some(name) = n.child_by_field_name("name") {
                    parts.push(format!("class {}", text(name, src)));
                }
            }
            "internal_module" | "module" => {
                if let Some(name) = n.child_by_field_name("name") {
                    parts.push(format!("namespace {}", text(name, src)));
                }
            }
            _ => {}
        }
        cur = n.parent();
    }
    parts.reverse();
    parts.join(" / ")
}

fn ts_is_exported(node: Node) -> bool {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if n.kind() == "export_statement" {
            return true;
        }
        if n.kind() == "program" {
            return false;
        }
        cur = n.parent();
    }
    false
}

/// The node whose prev-sibling holds the doc comment: the export_statement
/// wrapper when present, else the declaration (or lexical_declaration) itself.
fn ts_doc_anchor(node: Node) -> Node {
    let mut anchor = node;
    while let Some(p) = anchor.parent() {
        match p.kind() {
            "export_statement" | "lexical_declaration" | "variable_declaration" => anchor = p,
            _ => break,
        }
    }
    anchor
}

fn push_ts_fn(ex: &mut Extraction, node: Node, name_node: Node, body_node: Option<Node>, sig_from: Node, src: &str, file: &str, public: bool) {
    let full = text(sig_from, src);
    let sig_end = body_node
        .map(|b| b.start_byte().saturating_sub(sig_from.start_byte()))
        .unwrap_or(full.len())
        .min(full.len());
    let start = sig_from.start_position().row as u32 + 1;
    let end = sig_from.end_position().row as u32 + 1;
    ex.fns.push(FnRecord {
        name: text(name_node, src).to_string(),
        context: ts_context(node, src),
        sig: collapse_ws(&full[..sig_end]),
        doc: ts_doc_before(ts_doc_anchor(node), src),
        body: full.to_string(),
        file: file.to_string(),
        start,
        end,
        lines: end - start + 1,
        lang: "typescript".into(),
        public,
    });
}

fn walk_ts(node: Node, src: &str, file: &str, ex: &mut Extraction) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let body = child.child_by_field_name("body");
                    push_ts_fn(ex, child, name, body, child, src, file, ts_is_exported(child));
                }
            }
            "method_definition" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let body = child.child_by_field_name("body");
                    let private = text(child, src).trim_start().starts_with("private")
                        || text(name, src).starts_with('#');
                    push_ts_fn(ex, child, name, body, child, src, file, !private);
                }
            }
            "variable_declarator" => {
                if let (Some(name), Some(value)) =
                    (child.child_by_field_name("name"), child.child_by_field_name("value"))
                {
                    if matches!(value.kind(), "arrow_function" | "function_expression")
                        && name.kind() == "identifier"
                    {
                        let body = value.child_by_field_name("body");
                        push_ts_fn(ex, child, name, body, child, src, file, ts_is_exported(child));
                    }
                }
            }
            "interface_declaration" | "type_alias_declaration" | "enum_declaration"
            | "class_declaration" | "abstract_class_declaration" => {
                if let Some(name) = child.child_by_field_name("name") {
                    ex.types.push(TypeRecord {
                        name: text(name, src).to_string(),
                        kind: child.kind().trim_end_matches("_declaration").replace("_", " "),
                        doc: ts_doc_before(ts_doc_anchor(child), src),
                        file: file.to_string(),
                        start: child.start_position().row as u32 + 1,
                        lang: "typescript".into(),
                        public: ts_is_exported(child),
                    });
                }
            }
            _ => {}
        }
        walk_ts(child, src, file, ex);
    }
}

// -------------------------------------------------------- Functor Lang

/// Line-based: a top-level binding is `let name = ...` at column 0; its body
/// runs to the next top-level construct. Doc = contiguous `//` lines above.
fn extract_functor(src: &str, file: &str, ex: &mut Extraction) {
    let lines: Vec<&str> = src.lines().collect();
    let is_top_level =
        |l: &str| !l.starts_with(' ') && !l.starts_with('\t') && !l.trim().is_empty();

    let mut starts: Vec<(usize, String, bool)> = Vec::new(); // (line idx, name, is_type)
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("let ") {
            if let Some(name) = ident_prefix(rest) {
                starts.push((i, name, false));
            }
        } else if let Some(rest) = line.strip_prefix("type ") {
            if let Some(name) = ident_prefix(rest) {
                starts.push((i, name, true));
            }
        }
    }

    for (k, (i, name, is_type)) in starts.iter().enumerate() {
        // Body: to the line before the next top-level construct (not just the
        // next binding — any non-indented, non-comment line ends the body).
        let mut end = lines.len();
        for (j, l) in lines.iter().enumerate().skip(i + 1) {
            if is_top_level(l) && !l.trim_start().starts_with("//") {
                end = j;
                break;
            }
        }
        // Trim trailing blank/comment-only lines off the body.
        while end > i + 1
            && (lines[end - 1].trim().is_empty() || lines[end - 1].trim().starts_with("//"))
        {
            end -= 1;
        }
        let mut doc_parts = Vec::new();
        let mut j = *i;
        while j > 0 && lines[j - 1].trim_start().starts_with("//") {
            doc_parts.push(lines[j - 1].trim_start().trim_start_matches("//").trim());
            j -= 1;
        }
        doc_parts.reverse();

        if *is_type {
            ex.types.push(TypeRecord {
                name: name.clone(),
                kind: "type".into(),
                doc: doc_parts.join(" "),
                file: file.to_string(),
                start: *i as u32 + 1,
                lang: "functor".into(),
                public: true,
            });
            continue;
        }
        let body = lines[*i..end].join("\n");
        let _ = k;
        ex.fns.push(FnRecord {
            name: name.clone(),
            context: String::new(),
            sig: lines[*i].trim().to_string(),
            doc: doc_parts.join(" "),
            body,
            file: file.to_string(),
            start: *i as u32 + 1,
            end: end as u32,
            lines: (end - i) as u32,
            lang: "functor".into(),
            public: true,
        });
    }
}

fn ident_prefix(s: &str) -> Option<String> {
    let name: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let rest = s[name.len()..].trim_start();
    if !name.is_empty() && (rest.starts_with('=') || rest.starts_with(':')) {
        Some(name)
    } else {
        None
    }
}
