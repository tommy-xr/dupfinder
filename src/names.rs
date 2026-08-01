//! Lexical prior-art search: Jaccard similarity over identifier tokens.
//!
//! This is the cheap prefilter that sits between "grep the API index by hand"
//! and the embedding index. Splitting `find_nearest_cell` into
//! `{find, near, cell}` and intersecting token sets prunes thousands of items
//! to a handful of ranked candidates in milliseconds — no model, no `.dupfinder/`
//! store. It catches duplicates that *talk* alike; it cannot catch ones that only
//! *think* alike (that's what `similar`/`review` embeddings are for).

use crate::extract::Extraction;
use std::collections::BTreeSet;

/// Words that carry no signal about what a function does.
const STOPWORDS: &[&str] = &[
    "fn",
    "pub",
    "crate",
    "self",
    "mut",
    "ref",
    "impl",
    "dyn",
    "where",
    "const",
    "async",
    "await",
    "let",
    "return",
    "export",
    "function",
    "type",
    "interface",
    "class",
    "struct",
    "enum",
    "trait",
    "the",
    "a",
    "an",
    "of",
    "to",
    "for",
    "in",
    "on",
    "with",
    "and",
    "or",
];

/// Near-synonymous stems collapsed to one canonical token, so `find_closest`
/// and `get_nearest` land in the same bucket. Deliberately small: every entry
/// trades recall for precision, and the pairs below are the ones that actually
/// hide duplicates in practice.
const SYNONYMS: &[(&str, &str)] = &[
    ("closest", "near"),
    ("nearest", "near"),
    ("near", "near"),
    ("fetch", "get"),
    ("retrieve", "get"),
    ("read", "get"),
    ("lookup", "find"),
    ("search", "find"),
    ("locate", "find"),
    ("create", "make"),
    ("build", "make"),
    ("construct", "make"),
    ("new", "make"),
    ("delete", "remove"),
    ("erase", "remove"),
    ("drop", "remove"),
    ("convert", "to"),
    ("into", "to"),
    ("as", "to"),
    ("compute", "calc"),
    ("calculate", "calc"),
    ("update", "set"),
    ("write", "set"),
    ("check", "is"),
    ("has", "is"),
    ("verify", "is"),
];

fn canonical(tok: &str) -> &str {
    SYNONYMS
        .iter()
        .find(|(from, _)| *from == tok)
        .map_or(tok, |(_, to)| *to)
}

/// Split an identifier into canonical lowercase tokens: snake_case, camelCase,
/// PascalCase, `::` paths, digits, and hyphens all break.
pub fn tokenize(ident: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in ident.chars() {
        if ch.is_alphanumeric() {
            // camelCase / PascalCase boundary: lower|digit followed by upper.
            if ch.is_uppercase() && prev_lower && !cur.is_empty() {
                out.insert(std::mem::take(&mut cur));
            }
            cur.push(ch.to_ascii_lowercase());
            prev_lower = ch.is_lowercase() || ch.is_numeric();
        } else if !cur.is_empty() {
            out.insert(std::mem::take(&mut cur));
            prev_lower = false;
        }
    }
    if !cur.is_empty() {
        out.insert(cur);
    }
    out.into_iter()
        .filter(|t| t.len() > 1 && !STOPWORDS.contains(&t.as_str()))
        .map(|t| canonical(&t).to_string())
        .collect()
}

/// Type identifiers mentioned in a signature — the parameter and return types,
/// minus the function's own name. `fn load(p: &Path) -> Result<Mission>` yields
/// `{path, result, mission}`.
pub fn signature_tokens(sig: &str, own_name: &str) -> BTreeSet<String> {
    // Everything after the first '(' is params + return type.
    let tail = sig.split_once('(').map_or(sig, |(_, t)| t);
    let own = tokenize(own_name);
    tokenize(tail).difference(&own).cloned().collect()
}

pub fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f32;
    let union = a.union(b).count() as f32;
    inter / union
}

#[derive(Clone)]
pub struct Candidate {
    pub name: String,
    pub kind: &'static str,
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub doc: String,
    pub testish: bool,
    pub name_tokens: BTreeSet<String>,
    pub type_tokens: BTreeSet<String>,
}

impl Candidate {
    pub fn location(&self) -> String {
        format!("{}:{}", self.file, self.start)
    }
}

/// Name similarity dominates; matching types are corroboration, not proof —
/// `(&Mission) -> Vec<Cell>` is shared by plenty of unrelated helpers.
const NAME_WEIGHT: f32 = 0.75;
const TYPE_WEIGHT: f32 = 0.25;

pub fn score(a: &Candidate, b: &Candidate) -> (f32, f32, f32) {
    let n = jaccard(&a.name_tokens, &b.name_tokens);
    let t = jaccard(&a.type_tokens, &b.type_tokens);
    (NAME_WEIGHT * n + TYPE_WEIGHT * t, n, t)
}

pub fn candidates(ex: &Extraction) -> Vec<Candidate> {
    let mut out = Vec::with_capacity(ex.fns.len() + ex.types.len());
    for r in &ex.fns {
        out.push(Candidate {
            name: r.name.clone(),
            kind: "fn",
            file: r.file.clone(),
            start: r.start,
            end: r.end,
            doc: r.doc.clone(),
            testish: r.is_testish(),
            name_tokens: tokenize(&r.name),
            type_tokens: signature_tokens(&r.sig, &r.name),
        });
    }
    for t in &ex.types {
        out.push(Candidate {
            name: t.name.clone(),
            kind: match t.kind.as_str() {
                "struct" => "struct",
                "enum" => "enum",
                "trait" => "trait",
                _ => "type",
            },
            file: t.file.clone(),
            start: t.start,
            end: t.start,
            doc: t.doc.clone(),
            testish: false,
            name_tokens: tokenize(&t.name),
            type_tokens: BTreeSet::new(),
        });
    }
    out
}

/// A synthetic query for `--name foo` (prevention mode: nothing in git yet).
pub fn query_from_name(name: &str) -> Candidate {
    Candidate {
        name: name.to_string(),
        kind: "query",
        file: String::from("<query>"),
        start: 0,
        end: 0,
        doc: String::new(),
        testish: false,
        name_tokens: tokenize(name),
        type_tokens: BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn splits_snake_camel_and_pascal() {
        assert_eq!(
            tokenize("find_nearest_cell"),
            set(&["find", "near", "cell"])
        );
        assert_eq!(tokenize("findNearestCell"), set(&["find", "near", "cell"]));
        assert_eq!(tokenize("FindNearestCell"), set(&["find", "near", "cell"]));
        assert_eq!(
            tokenize("path::to::load_mission"),
            set(&["path", "load", "mission"])
        );
    }

    #[test]
    fn drops_stopwords_and_single_chars() {
        assert_eq!(tokenize("pub fn a_b_parse"), set(&["parse"]));
    }

    #[test]
    fn synonyms_collapse() {
        // The whole point: different words, same concept, identical token set.
        assert_eq!(tokenize("get_closest_node"), tokenize("fetch_nearest_node"));
        assert_eq!(tokenize("create_entity"), tokenize("build_entity"));
        assert!(
            jaccard(
                &tokenize("find_closest_cell"),
                &tokenize("lookup_nearest_cell")
            ) > 0.99
        );
    }

    #[test]
    fn jaccard_is_bounded_and_symmetric() {
        let a = tokenize("load_mission_file");
        let b = tokenize("load_mission");
        assert!((jaccard(&a, &a) - 1.0).abs() < f32::EPSILON);
        assert_eq!(jaccard(&a, &b), jaccard(&b, &a));
        assert!((jaccard(&a, &b) - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(jaccard(&a, &BTreeSet::new()), 0.0);
        assert_eq!(jaccard(&tokenize("alpha"), &tokenize("beta")), 0.0);
    }

    #[test]
    fn signature_tokens_exclude_own_name() {
        let toks = signature_tokens(
            "fn load_mission(p: &Path) -> Result<Mission>",
            "load_mission",
        );
        assert!(toks.contains("path"));
        assert!(toks.contains("result"));
        // "mission" is part of the fn's own name, so it is not extra evidence.
        assert!(!toks.contains("mission"));
        assert!(!toks.contains("load"));
    }

    #[test]
    fn unrelated_names_score_zero() {
        let a = query_from_name("parse_gamesys_header");
        let b = query_from_name("render_hud_overlay");
        assert_eq!(score(&a, &b).0, 0.0);
    }
}
