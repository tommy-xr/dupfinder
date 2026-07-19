//! Changed-line ranges for the working tree vs a base ref, via `git diff -U0`.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

pub type ChangedRanges = BTreeMap<String, Vec<(u32, u32)>>;

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .context("run git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn resolve_base(root: &Path, explicit: Option<String>) -> Result<String> {
    if let Some(b) = explicit {
        return Ok(b);
    }
    for candidate in ["origin/main", "origin/master", "main", "master"] {
        if git(root, &["rev-parse", "--verify", "--quiet", candidate]).is_ok() {
            return Ok(candidate.to_string());
        }
    }
    bail!("no base ref found (tried origin/main, origin/master, main, master) — pass --base");
}

/// Ranges of NEW/CHANGED lines per file: merge-base(base, HEAD) -> working tree,
/// plus whole-file ranges for untracked files.
pub fn changed_ranges(root: &Path, base: &str) -> Result<ChangedRanges> {
    // `git diff A...` diffs merge-base(A, HEAD) against the working tree, so
    // committed and uncommitted edits are both in scope.
    let diff = git(root, &["diff", "-U0", "--no-color", &format!("{base}...")])?;
    let mut ranges: ChangedRanges = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            current = Some(path.to_string());
        } else if line.starts_with("+++ ") {
            current = None; // deleted file (+++ /dev/null)
        } else if let (Some(file), true) = (&current, line.starts_with("@@")) {
            // @@ -a[,b] +c[,d] @@ — take the +c,d (new-file) side.
            if let Some(plus) = line.split_whitespace().find(|t| t.starts_with('+')) {
                let mut it = plus[1..].splitn(2, ',');
                let start: u32 = it.next().unwrap_or("0").parse().unwrap_or(0);
                let len: u32 = it.next().map_or(1, |s| s.parse().unwrap_or(1));
                if len > 0 && start > 0 {
                    ranges
                        .entry(file.clone())
                        .or_default()
                        .push((start, start + len - 1));
                }
            }
        }
    }
    let untracked = git(root, &["ls-files", "--others", "--exclude-standard"])?;
    for f in untracked.lines().filter(|l| !l.is_empty()) {
        ranges.entry(f.to_string()).or_default().push((1, u32::MAX));
    }
    Ok(ranges)
}

pub fn overlaps(ranges: &[(u32, u32)], start: u32, end: u32) -> bool {
    ranges.iter().any(|&(a, b)| !(b < start || a > end))
}
