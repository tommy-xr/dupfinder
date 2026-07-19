---
name: review-for-duplicates
description: >-
  Duplication detection and reuse discovery via the dupfinder CLI. Two moments:
  PREVENTION — before writing a new function/helper, or when asked "does
  something already exist for X?", grep the `dupfinder index` API summary so a
  duplicate is never written. DETECTION — `dupfinder review` checks a change
  for token-level copy-paste clones (jscpd) plus embedding-based semantic
  similarity ("this new function closely resembles an existing one — reuse
  it?"). Use when about to add a helper, when checking a change or codebase for
  duplication, before extracting shared helpers, or as the duplication step
  inside a larger review. Also supports whole-repo audits via `dupfinder similar`.
---

# review-for-duplicates

Evidence-driven duplication review. The `dupfinder` CLI supplies deterministic
evidence (token clones + embedding neighbors); your job is judgment — deciding
which findings warrant reuse/extraction and which duplication is acceptable.

dupfinder serves two moments, and both matter:

- **Prevention (before writing code)** — `dupfinder index` emits a greppable
  summary of every existing function/type. Grepping it for the domain terms of
  the helper you're about to write is the highest-leverage move: it stops a
  duplicate from being written at all, rather than catching it afterward. See
  [Prevention](#prevention--before-writing-new-code) below. Reach for this
  whenever you're about to add a helper, or are asked "is there already
  something for X?".
- **Detection (after writing code)** — `dupfinder review` compares a change
  against the codebase. This is the bulk of the skill (Steps 1–3).

This skill ships with dupfinder itself. Install/update it with
`dupfinder install-skill` (writes to `~/.claude/skills/`) or
`dupfinder install-skill --project` (writes to `./.claude/skills/`).

## Prevention — before writing new code

Before adding a function/helper (or when asked what already exists), generate
the API index and grep it for the concepts you're about to implement:

```sh
dupfinder index <repo-root> --out /tmp/api-index.md
grep -i -E 'atomic|temp.?file|rename' /tmp/api-index.md   # terms of the helper you're about to write
```

Each hit is `signature — doc first-sentence  (file:line)`. If something already
does the job, use or extend it instead of writing a new one — and say so. The
index is cheap (no embeddings, just extraction), so run it freely. This is the
single most effective step against re-implementing existing logic; prefer it
over writing-then-detecting whenever you know new code is coming.

## Step 0 — Locate dupfinder

```sh
DF=$(command -v dupfinder)
[ -n "$DF" ] || echo "dupfinder not on PATH — install with: cargo install --path <dupfinder-checkout>"
```

If it isn't installed and can't be, fall back to plain jscpd
(`npx jscpd --min-tokens 70`) for the token pass and say the semantic pass was
skipped.

Notes:
- The first embedding run downloads the model (~600MB, one-time) and a cold
  index embeds every function — on a large repo give it several minutes (run
  with a generous timeout, in the background if long). Subsequent runs are
  incremental and fast.
- `review`/`similar` build or refresh the index automatically; no separate
  `embed` step is needed.

## Step 1 — Run the review

```sh
dupfinder review <repo-root> --base <BASE> > /tmp/dupfinder-review.md
```

`--base` should be the change's stack parent (the PR base ref if one exists,
else origin/main). Omit it to auto-resolve (origin/main, origin/master, main,
master — first that exists). The report has two sections:

1. **Token clones touching this change (jscpd)** — copy-paste evidence.
2. **Similar existing code per changed function (embeddings)** — for each
   changed function, its nearest neighbors in the codebase.

## Step 2 — Judge the evidence

Read the report and, for every listed neighbor/clone, decide by **reading both
sides** (open the cited file:line):

- **Genuine duplication** — the changed code re-implements the neighbor.
  Finding: recommend reusing/extending the existing helper (name it and its
  location), or extracting a shared one. Severity by blast radius: shared-seam
  logic that must stay in sync (e.g. across platform targets) is High; a local
  convenience copy is Medium/Low.
- **Acceptable near-duplication** — intentional API families (`cube`/`sphere`/
  `quad` builders), platform twins that *must* differ, boilerplate the
  language forces. Say why it's acceptable; no finding.
- **Similarity scores**: pair-mode 0.90+ = strong signal; query-mode (the
  review report) is rank-meaningful only — a true re-implementation's top
  neighbor often scores 0.5–0.7. Never dismiss a neighbor for a "low" score
  without reading it; never report one as duplication without reading it.
- Neighbors tagged `[also changed in this diff]` mean the PR introduces the
  same logic twice — the strongest finding type.
- Neighbors tagged `[test code]` rarely warrant extraction; flag only if the
  duplicated logic is nontrivial.

## Step 3 — Report

A short markdown list, strongest first. Each finding: the changed function
(file:line), what it duplicates (file:line), the evidence (clone lines or
similarity + your read), and the concrete recommendation (reuse X / extract to
Y / accept because Z). If nothing survives judgment, say "no actionable
duplication" plainly — with the scan sizes so it's clear the check ran.

## Whole-repo audit

Asked to "find duplication in the codebase" (no specific change in scope):
`dupfinder similar <root> --threshold 0.9 --top 40` and judge the pairs the same
way as Step 2. For prevention/reuse-discovery before writing new code, see
[Prevention](#prevention--before-writing-new-code) above.
