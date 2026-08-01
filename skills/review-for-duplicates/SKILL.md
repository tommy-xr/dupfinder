---
name: review-for-duplicates
description: >-
  Duplication detection and reuse discovery via the dupfinder CLI, scoped to a
  change and fast enough to run every time. Two moments: PREVENTION — before
  writing a new function/helper, or when asked "does something already exist for
  X?", `dupfinder names --name <ident>` ranks existing prior art (and
  `dupfinder index` is the greppable full list). DETECTION — `dupfinder names
  --base <ref>` plus `dupfinder clones` check a change for lexically similar
  existing code and token-level copy-paste. Both passes are seconds and use no
  embeddings. Use when about to add a helper, when checking a change for
  duplication, or as the duplication step inside a larger review. For a
  whole-repo audit, use the heavier `audit-duplicates` skill instead.
---

# review-for-duplicates

Evidence-driven duplication review of **a change**. dupfinder supplies
deterministic evidence; your job is judgment — which findings warrant reuse or
extraction, and which duplication is fine.

**Scope: duplication only.** Correctness, security, performance, and style are
out of scope here; other reviewers cover those.

Everything in this skill is **embeddings-free and takes seconds**, so it can run
on every change. The embedding engine (`dupfinder similar` / `review`) is
deliberately not used — a cold index costs minutes. When you specifically want
semantic duplication across a whole repo, use the **`audit-duplicates`** skill,
and only on an explicit request.

This skill ships with dupfinder itself. Install/update it with
`dupfinder install-skill` (writes to `~/.claude/skills/`) or
`dupfinder install-skill --project` (writes to `./.claude/skills/`).

## Prevention — before writing new code

The highest-leverage move: stop the duplicate from being written at all.

```sh
# ranked prior art for the helper you are about to write
dupfinder names <repo-root> --name parse_manifest_header

# or the full greppable API list, when you'd rather scan by domain term
dupfinder index <repo-root> --out /tmp/api-index.md
grep -i -E 'atomic|temp.?file|rename' /tmp/api-index.md
```

`names --name` splits the identifier into tokens, collapses synonym stems
(`closest`~`nearest`, `fetch`~`get`, `build`~`create`), and ranks existing
functions/types by overlap — so `find_nearest_cell` surfaces
`find_closest_reachable_cell` even though they share only one literal word. Each
hit is `score [name/types] kind name file:line — doc`.

If something already does the job, use or extend it, and say so.

## Step 0 — Locate dupfinder

```sh
DF=$(command -v dupfinder)
[ -n "$DF" ] || echo "dupfinder not on PATH — install with: cargo install --path <dupfinder-checkout>"
```

If it isn't installed and can't be, fall back to `npx jscpd --min-tokens 70` for
the token pass, grep for prior art by hand, and say which passes were skipped.

## Step 1 — Gather evidence

```sh
BASE=<the change's stack parent: the PR base ref if one exists, else origin/main>

# lexical prior art for what the change touches
"$DF" names <repo-root> --base "$BASE" > /tmp/dup-names.txt

# token-level copy-paste, whole-repo (NOT diff-scoped — filter it yourself)
"$DF" clones <repo-root> > /tmp/dup-clones.txt
```

`--base` omitted auto-resolves to origin/main, origin/master, main, or master —
first that exists. In a stacked-PR workflow pass the parent branch explicitly, or
the review covers the whole stack.

Two notes that change how you read the output:

- `names --base` diffs **merge-base(base, HEAD) → working tree**, so uncommitted
  edits are included. It queries every fn/type *overlapping* changed lines, which
  is slightly broader than "what this change added".
- `clones` scans the whole repo. **Filter it to the changed files** and discard
  pairs that don't touch the change.

## Step 2 — Judge the evidence

Open both `file:line`s before calling anything a duplicate. Scores order your
reading; they never conclude for you.

- **Genuine duplication** — the changed code re-implements existing code.
  Recommend reusing/extending it (name it and its location) or extracting a
  shared helper. Severity by blast radius: logic that must stay in sync across
  seams (platform targets, crates) is High; a local convenience copy is Low.
- **Acceptable near-duplication** — intentional API families (`cube`/`sphere`/
  `quad`), inverse pairs (`vec3_to_point3`/`point3_to_vec3`), platform twins that
  must differ, language-forced boilerplate. Say why; no finding.
- **Structurally forced** — trait/interface impls and overrides share names
  because the trait dictates them. `names` already drops same-name trait impls
  and pure forwarding methods; `clones` does not.
- **Diverged duplicates** are the strongest finding: two implementations of one
  idea whose behavior differs (e.g. one handles Unicode, the other only ASCII).
  That is a live bug, not just redundancy.
- **Blind spot to cover yourself:** this is all token overlap. A re-implementation
  sharing no vocabulary scores 0.00 and will not appear. Skim the change for
  logic you recognize from elsewhere.

## Step 3 — Report

One line per finding, strongest first:

```
<tag> <what to cut>. <what to use instead>. [path:line <-> path:line]
```

Tags: `merge` (two impls of one idea), `reuse` (call the existing helper),
`extract` (both move to a shared home), `diverged` (behavior differs — probable
bug).

State what ran, so coverage is legible: the base ref, the number of queries and
candidates, and whether the clone pass was available.

If nothing survives judgment, say exactly that — **"No actionable duplication."**
with the scan sizes. A clean result is a real result; never pad it.
