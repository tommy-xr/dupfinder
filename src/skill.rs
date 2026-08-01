//! `dupfinder install-skill` — write the bundled skills (embedded at compile
//! time) into a Claude Code skills directory.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// The bundled skills, as (directory name, contents). `review-for-duplicates`
/// is the cheap per-change pass; `audit-duplicates` is the heavy whole-repo one.
const SKILLS: &[(&str, &str)] = &[
    (
        "review-for-duplicates",
        include_str!("../skills/review-for-duplicates/SKILL.md"),
    ),
    (
        "audit-duplicates",
        include_str!("../skills/audit-duplicates/SKILL.md"),
    ),
];

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

    for (name, body) in SKILLS {
        let skill_dir = skills_root.join(name);
        let target = skill_dir.join("SKILL.md");
        let existed = target.exists();
        std::fs::create_dir_all(&skill_dir)
            .with_context(|| format!("create {}", skill_dir.display()))?;
        std::fs::write(&target, body).with_context(|| format!("write {}", target.display()))?;

        println!(
            "{} skill '{}' at {}",
            if existed { "Updated" } else { "Installed" },
            name,
            target.display()
        );
    }
    Ok(())
}
