//! Token-level copy-paste clones via jscpd (shelled out through npx).
//! jscpd is optional: if npx is unavailable the caller gets Ok(None) and the
//! review proceeds on embeddings alone.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub struct TokenClone {
    pub file_a: String,
    pub a: (u32, u32),
    pub file_b: String,
    pub b: (u32, u32),
    pub lines: u32,
}

impl TokenClone {
    /// jscpd reports paths relative to each scanned root, so match on a
    /// path-component-anchored suffix ("store.rs" must not match "restore.rs").
    pub fn touches(&self, repo_rel_file: &str) -> bool {
        fn suffix_match(a: &str, b: &str) -> bool {
            a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}"))
        }
        suffix_match(repo_rel_file, &self.file_a) || suffix_match(repo_rel_file, &self.file_b)
    }
}

pub fn run_jscpd(root: &Path) -> Result<Option<Vec<TokenClone>>> {
    if Command::new("npx").arg("--version").output().is_err() {
        eprintln!("[dupfinder] npx not found — skipping token-clone (jscpd) pass");
        return Ok(None);
    }
    let out_dir = std::env::temp_dir().join(format!("dupfinder-jscpd-{}", std::process::id()));
    let mut cmd = Command::new("npx");
    cmd.current_dir(root)
        .arg("--yes")
        .arg("jscpd")
        .args(["--reporters", "json", "--silent", "--output"])
        .arg(&out_dir);
    // Honor the repo's own jscpd config when present; otherwise sane defaults.
    if !root.join(".jscpd.json").exists() {
        // jscpd >=5 respects .gitignore by default (it only offers --no-gitignore),
        // and rejects the old --gitignore flag outright.
        cmd.args(["--min-tokens", "70", "--ignore"]).arg(
            "**/node_modules/**,**/target/**,**/dist/**,**/build/**,**/.git/**,**/*.min.js,**/*.json,**/*.md",
        );
        cmd.arg(".");
    }
    let output = cmd.output().context("run npx jscpd")?;
    let report_path = out_dir.join("jscpd-report.json");
    if !report_path.exists() {
        eprintln!(
            "[dupfinder] jscpd produced no report — skipping token-clone pass\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(None);
    }
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path)?).context("parse jscpd report")?;
    let _ = std::fs::remove_dir_all(&out_dir);

    let mut clones = Vec::new();
    for d in report["duplicates"].as_array().into_iter().flatten() {
        let side = |k: &str| -> Option<(String, (u32, u32))> {
            let f = &d[k];
            Some((
                f["name"].as_str()?.replace('\\', "/"),
                (f["start"].as_u64()? as u32, f["end"].as_u64()? as u32),
            ))
        };
        if let (Some((file_a, a)), Some((file_b, b))) = (side("firstFile"), side("secondFile")) {
            clones.push(TokenClone {
                file_a,
                a,
                file_b,
                b,
                lines: d["lines"].as_u64().unwrap_or(0) as u32,
            });
        }
    }
    Ok(Some(clones))
}
