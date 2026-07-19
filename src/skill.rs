//! `dupfinder install-skill` — write the bundled review-for-duplicates skill
//! (embedded at compile time) into a Claude Code skills directory.

use anyhow::{Context, Result};
use std::path::PathBuf;

const SKILL_NAME: &str = "review-for-duplicates";
const SKILL_BODY: &str = include_str!("../skills/review-for-duplicates/SKILL.md");

/// Where the skill gets written. `project` => <cwd or given dir>/.claude/skills;
/// otherwise the user-global ~/.claude/skills.
pub fn install(project: bool, dir: Option<PathBuf>) -> Result<()> {
    let skills_root = if project {
        dir.unwrap_or_else(|| PathBuf::from(".")).join(".claude/skills")
    } else if let Some(d) = dir {
        d
    } else {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .context("no HOME/USERPROFILE set — pass an explicit --dir")?;
        PathBuf::from(home).join(".claude/skills")
    };

    let skill_dir = skills_root.join(SKILL_NAME);
    let target = skill_dir.join("SKILL.md");
    let existed = target.exists();
    std::fs::create_dir_all(&skill_dir)
        .with_context(|| format!("create {}", skill_dir.display()))?;
    std::fs::write(&target, SKILL_BODY)
        .with_context(|| format!("write {}", target.display()))?;

    println!(
        "{} skill '{}' at {}",
        if existed { "Updated" } else { "Installed" },
        SKILL_NAME,
        target.display()
    );
    Ok(())
}
