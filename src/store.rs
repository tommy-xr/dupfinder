//! The .dupfinder/ index: extracted functions + their embedding vectors,
//! rebuilt incrementally (only files whose content hash changed re-embed).

use crate::extract::{self, FnRecord};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DIR: &str = ".dupfinder";
const MODEL: &str = "jinaai/jina-embeddings-v2-base-code";

pub struct Store {
    pub records: Vec<FnRecord>,
    /// Normalized vectors, records[i] <-> vectors[i].
    pub vectors: Vec<Vec<f32>>,
    pub file_hashes: BTreeMap<String, String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Meta {
    model: String,
    dim: usize,
    count: usize,
    file_hashes: BTreeMap<String, String>,
}

fn dir(root: &Path) -> PathBuf {
    root.join(DIR)
}

impl Store {
    pub fn load(root: &Path) -> Result<Option<Store>> {
        let d = dir(root);
        let meta_path = d.join("meta.json");
        if !meta_path.exists() {
            return Ok(None);
        }
        let meta: Meta = serde_json::from_str(&std::fs::read_to_string(&meta_path)?)
            .context("parse meta.json")?;
        if meta.model != MODEL {
            return Ok(None);
        }
        let records: Vec<FnRecord> = std::fs::read_to_string(d.join("functions.jsonl"))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()
            .context("parse functions.jsonl")?;
        let bytes = std::fs::read(d.join("embeddings.bin"))?;
        if records.len() != meta.count || bytes.len() != meta.count * meta.dim * 4 {
            bail!("index is corrupt (size mismatch) — delete {} and re-run embed", d.display());
        }
        let vectors = bytes
            .chunks_exact(meta.dim * 4)
            .map(|chunk| {
                chunk
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect()
            })
            .collect();
        Ok(Some(Store {
            records,
            vectors,
            file_hashes: meta.file_hashes,
        }))
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let d = dir(root);
        std::fs::create_dir_all(&d)?;
        let dim = self.vectors.first().map_or(0, |v| v.len());
        let mut jsonl = String::new();
        for r in &self.records {
            jsonl.push_str(&serde_json::to_string(r)?);
            jsonl.push('\n');
        }
        std::fs::write(d.join("functions.jsonl"), jsonl)?;
        let mut bin = std::fs::File::create(d.join("embeddings.bin"))?;
        for v in &self.vectors {
            for x in v {
                bin.write_all(&x.to_le_bytes())?;
            }
        }
        let meta = Meta {
            model: MODEL.into(),
            dim,
            count: self.records.len(),
            file_hashes: self.file_hashes.clone(),
        };
        std::fs::write(d.join("meta.json"), serde_json::to_string_pretty(&meta)?)?;
        // The index is derived state; keep it out of the consumer's repo.
        let _ = std::fs::write(d.join(".gitignore"), "*\n");
        Ok(())
    }
}

/// Extract the tree, reuse vectors for unchanged files, embed the rest.
pub fn build_or_update(root: &Path) -> Result<Store> {
    let ex = extract::extract_dir(root)?;
    let old = Store::load(root).unwrap_or(None);

    // Map old vectors by file for files whose hash is unchanged.
    let mut reusable: BTreeMap<&str, Vec<(&FnRecord, &Vec<f32>)>> = BTreeMap::new();
    if let Some(old) = &old {
        for (r, v) in old.records.iter().zip(&old.vectors) {
            if old.file_hashes.get(&r.file) == ex.file_hashes.get(&r.file) {
                reusable.entry(&r.file).or_default().push((r, v));
            }
        }
    }

    let mut vectors: Vec<Option<Vec<f32>>> = vec![None; ex.fns.len()];
    let mut to_embed: Vec<usize> = Vec::new();
    let mut per_file_seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, r) in ex.fns.iter().enumerate() {
        let k = per_file_seen.entry(r.file.as_str()).or_insert(0);
        // Same content hash => extraction is deterministic => same records in
        // the same order, so pair by position within the file.
        if let Some(list) = reusable.get(r.file.as_str()) {
            if let Some((_, v)) = list.get(*k) {
                vectors[i] = Some((*v).clone());
            }
        }
        *k += 1;
        if vectors[i].is_none() {
            to_embed.push(i);
        }
    }

    if !to_embed.is_empty() {
        eprintln!(
            "[dupfinder] embedding {} of {} functions ({} reused)",
            to_embed.len(),
            ex.fns.len(),
            ex.fns.len() - to_embed.len()
        );
        // Checkpoint groups of ~250 fns, split only at file boundaries so a
        // partial store always covers whole files — the hash-based reuse above
        // then resumes an interrupted run instead of starting over.
        let mut groups: Vec<Vec<usize>> = vec![Vec::new()];
        for &i in &to_embed {
            let cur = groups.last_mut().unwrap();
            let file_boundary =
                cur.last().is_none_or(|&p| ex.fns[p].file != ex.fns[i].file);
            if cur.len() >= 250 && file_boundary {
                groups.push(Vec::new());
            }
            groups.last_mut().unwrap().push(i);
        }
        let total = to_embed.len();
        let mut done = 0usize;
        for group in groups {
            let texts: Vec<String> = group.iter().map(|&i| ex.fns[i].embed_text()).collect();
            let embedded = crate::embedder::embed(texts)?;
            done += group.len();
            for (&slot, v) in group.iter().zip(embedded) {
                vectors[slot] = Some(v);
            }
            if done < total {
                checkpoint(root, &ex, &vectors)?;
                eprintln!("[dupfinder] {done}/{total} embedded (checkpoint saved)");
            }
        }
    }

    let store = Store {
        records: ex.fns,
        vectors: vectors.into_iter().map(|v| v.unwrap()).collect(),
        file_hashes: ex.file_hashes,
    };
    store.save(root)?;
    Ok(store)
}

/// Save a valid partial store: only records from files whose every function
/// has a vector, with file_hashes restricted to those files.
fn checkpoint(root: &Path, ex: &extract::Extraction, vectors: &[Option<Vec<f32>>]) -> Result<()> {
    let mut file_ok: BTreeMap<&str, bool> = BTreeMap::new();
    for (r, v) in ex.fns.iter().zip(vectors) {
        let e = file_ok.entry(r.file.as_str()).or_insert(true);
        *e &= v.is_some();
    }
    let mut records = Vec::new();
    let mut vecs = Vec::new();
    for (r, v) in ex.fns.iter().zip(vectors) {
        if file_ok.get(r.file.as_str()) == Some(&true) {
            records.push(r.clone());
            vecs.push(v.clone().unwrap());
        }
    }
    let hashes = ex
        .file_hashes
        .iter()
        .filter(|(f, _)| file_ok.get(f.as_str()).copied().unwrap_or(true))
        .map(|(f, h)| (f.clone(), h.clone()))
        .collect();
    Store {
        records,
        vectors: vecs,
        file_hashes: hashes,
    }
    .save(root)
}
