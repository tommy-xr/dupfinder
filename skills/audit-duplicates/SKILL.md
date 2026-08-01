---
name: audit-duplicates
description: >-
  Whole-repo duplication audit via the dupfinder CLI — the heavy, occasional
  counterpart to `review-for-duplicates` (which is diff-scoped and cheap). Runs
  all three engines over an entire codebase: lexical name similarity
  (`names --all`), token copy-paste clones (`clones`), and embedding-based
  semantic similarity (`similar`), then judges the survivors into a ranked
  cleanup list. Use ONLY when explicitly asked to audit a repo for duplication
  ("audit X for duplicates", "where is the duplication in this codebase") — it
  costs minutes on a cold repo and is not part of ordinary review.
---

# audit-duplicates

A standing duplication audit of an entire codebase. `review-for-duplicates` asks
"does this *change* duplicate something?" — this asks "what does this *repo*
duplicate?", and answers it with every engine dupfinder has.

**Run this only on an explicit request.** It is not a review step, not a
pre-commit check, and must never be triggered automatically as part of another
task. It reads the whole repo and, on a cold embedding index, takes minutes.

**Scope: duplication only.** Correctness bugs, security issues, performance, and
style are explicitly out of scope — if you notice one, mention it in a single
closing line, but do not let it turn this into a general code review.

**One-shot report. Do not apply fixes.** Produce the ranked list and stop, unless
separately asked to act on it.

## Step 0 — Locate dupfinder and set the scope

```sh
DF=$(command -v dupfinder)
[ -n "$DF" ] || echo "dupfinder not on PATH — install with: cargo install --path <dupfinder-checkout>"
ROOT=<repo root>            # literal path; never guess from cwd inside a worktree
```

Decide exclusions **before** running. The dominant false-positive class is
directories whose contents are *supposed* to repeat each other:

- self-contained examples/demos (`examples/**`, `site/demos/**`) — each one
  redefines the same small helpers on purpose
- generated code, vendored code, fixtures/corpora

Pass each as `--exclude <glob>` (repeatable). On functor this is the difference
between 2256 and 809 pairs. If unsure whether a directory qualifies, run once
without exclusions, look at what dominates the top, and re-run.

## Step 1 — Run all three engines

Lexical and token passes are seconds; run them first so there's something to
work with while the embedding pass warms up.

```sh
# 1. lexical — names that talk alike (fast, no model)
"$DF" names "$ROOT" --all --min-score 0.5 --top 40 \
  --exclude 'examples/**' > /tmp/audit-names.txt

# 2. token — literal copy-paste (fast, needs npx)
"$DF" clones "$ROOT" > /tmp/audit-clones.txt

# 3. semantic — same logic, different vocabulary (SLOW on a cold index:
#    downloads a ~600MB model once, then embeds every function. Minutes.
#    Run it in the background with a generous timeout and collect it later.)
"$DF" similar "$ROOT" --threshold 0.9 --top 40 > /tmp/audit-similar.txt
```

The three engines have deliberately different blind spots, which is the whole
reason to run all of them here:

| Engine | Finds | Misses |
| --- | --- | --- |
| `names --all` | shared vocabulary (`find_nearest` vs `find_closest`) | rewrites sharing no words |
| `clones` | literal copied text | anything renamed or reformatted |
| `similar` | same logic, different names | logic that only *looks* alike |

A pair surfaced by **two or more** engines is the strongest signal available —
mark those first.

## Step 2 — Judge, by reading both sides

Every candidate is a hypothesis until you open both `file:line`s. Read them.
Ranked scores are for ordering your reading, never for concluding.

Discard, without reporting:

- **Structurally forced** — trait/interface implementations, overrides, and
  language boilerplate that *must* share a shape. (`names --all` already drops
  same-name trait impls and pure forwarding methods; `clones` and `similar` do
  not.)
- **Intentional API families** — `translate_x`/`translate_y`/`translate_z`,
  `cube`/`sphere`/`quad`. Symmetry is the point; collapsing them is worse.
- **Inverse pairs** — `vec3_to_point3` / `point3_to_vec3`.
- **Platform twins that must differ** — desktop/web/mobile variants.
- **Test scaffolding**, unless the duplicated logic is substantial.

Keep, and investigate: two implementations of the same *idea* in different
modules or crates, especially when they **disagree**. Divergence is the strongest
evidence that duplication is a live bug and not just noise — e.g. two
`capitalize(&str) -> String` helpers where one handles any Unicode first
character and the other only ASCII will silently behave differently on the same
input.

## Step 3 — Report, biggest cut first

Rank by **payoff** (lines removable × blast radius), not by the tool's score. A
0.75 pair spanning two crates beats a 0.92 pair inside one file.

One line per finding:

```
<tag> <what to cut>. <what to use instead>. [path:line <-> path:line]
```

Tags: `merge` (two impls of one idea — pick one), `reuse` (caller should call the
existing helper), `extract` (both should move to a shared home), `diverged`
(duplicates whose behavior differs — flag as a probable bug).

Close with a summary line:

```
net: -<N> lines across <M> sites; <K> pairs confirmed by 2+ engines.
```

State what you excluded and what each engine scanned, so the coverage is legible.
If the embedding pass was still running, say so rather than implying it ran.

If nothing survives judgment, say exactly that — **"No actionable duplication."**
plus the scan sizes. A clean audit is a real result; do not pad it with
speculative findings to look thorough.
