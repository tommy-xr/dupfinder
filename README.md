# dupfinder

Duplication detection and reuse discovery for **Rust**, **TypeScript/JavaScript**, and
**[Functor Lang](https://github.com/tommy-xr/functor)** — built for LLM-assisted code review,
where the common failure mode is *re-implementing a helper that already exists, written just
differently enough that token-based clone detectors can't see it*.

Four engines in one CLI:

| Engine | Catches | Command |
| --- | --- | --- |
| **API index** | nothing — *prevents*: a greppable summary of every public function/type, so authors (human or LLM) can find the canonical helper before writing a new one | `dupfinder index` |
| **Name Jaccard** | prior art that *talks* alike — `find_nearest_cell` vs `find_closest_reachable_cell`. Pure string work: no model, no index, milliseconds | `dupfinder names` |
| **Code embeddings** (jina-embeddings-v2-base-code via [fastembed](https://github.com/Anush008/fastembed-rs)/ONNX, fully local) | semantic duplication — same logic, different names/style | `dupfinder embed` / `similar` / `review` |
| **[jscpd](https://github.com/kucherenko/jscpd)** (via `npx`, optional) | token-level copy-paste clones | `dupfinder clones` / `review` |

## Install

```sh
cargo install --path .
```

The first `embed`/`similar`/`review` run downloads the embedding model (~600MB, one-time)
to the fastembed cache; everything runs offline after that. `npx` (Node) is only needed
for the jscpd pass — without it, `review` degrades gracefully to embeddings-only.

## Commands

```sh
dupfinder index [DIR] [--private] [--out api-index.md]
    # markdown API summary: every public fn/type with signature, doc first-sentence, file:line

dupfinder embed [DIR]
    # build/update the embedding index in DIR/.dupfinder/ (incremental: only
    # files whose content hash changed are re-embedded)

dupfinder similar [DIR] [--threshold 0.9] [--top 40] [--min-lines 6] [--include-tests]
    # most similar function pairs across the whole index — a duplication audit

dupfinder clones [DIR]
    # token-level copy-paste clones (jscpd); honors DIR/.jscpd.json if present

dupfinder names [DIR] [--base origin/main] [--name IDENT]... [--all] [--exclude GLOB]...
              [--top 5] [--min-score 0.3] [--include-tests]
    # lexical prior art: rank existing fns/types by identifier-token (Jaccard)
    # similarity to what the change adds. Splits snake/camel/Pascal, drops
    # stopwords, collapses synonym stems (closest~nearest, fetch~get, build~create),
    # and scores 0.75*name + 0.25*signature-type overlap.
    # --name scores a bare identifier with no diff — use it BEFORE writing the
    # function. No embeddings, so it runs in milliseconds on any repo size.
    # --all audits the WHOLE repo instead of a diff: every pair ranked once,
    # damped by how distinctive the shared words are (so `new` vs `new` sinks),
    # with trait-dictated names and pure forwarding methods excluded.
    # --exclude skips files by glob, repeatable — 'examples/**' matters for repos
    # whose examples are deliberately self-contained (functor: 2256 -> 809 pairs).

dupfinder review [DIR] [--base origin/main] [--top 3] [--min-lines 5]
    # review the current change: token clones touching the diff + the most
    # similar existing functions for each changed function. Base defaults to
    # origin/main / origin/master / main / master, first that exists.

dupfinder install-skill [--project] [--dir DIR]
    # install the bundled Claude Code skills (review-for-duplicates, audit-duplicates).
    # default target ~/.claude/skills; --project writes ./.claude/skills;
    # --dir overrides the location. Re-run to update.
```

`review` is the CI/review-time entry point: its markdown output is designed to be handed
to a reviewer (human or LLM) as evidence — "this changed function closely resembles X;
should it reuse it?"

## Claude Code skills

dupfinder bundles two [Claude Code](https://claude.com/claude-code) skills, split by
cost so the cheap one can run on every change:

| Skill | Scope | Engines | Cost | When |
| --- | --- | --- | --- | --- |
| `review-for-duplicates` | one change | `names --base`, `names --name`, `clones` | seconds, no model | every change, and before writing a new helper |
| `audit-duplicates` | whole repo | adds `similar` (embeddings) | minutes on a cold repo | only on an explicit request |

Install both after `cargo install`:

```sh
dupfinder install-skill              # ~/.claude/skills (all projects)
dupfinder install-skill --project    # ./.claude/skills (this repo only)
```

The skill sources live in [`skills/`](skills/) and are embedded in the binary, so
`install-skill` always writes the versions matching your installed `dupfinder`.

## Reading similarity numbers

- **Pair mode** (`similar`): 0.90+ is a strong duplication signal.
- **Query mode** (`review`): scores are *rank*-meaningful, not absolute — a genuine
  re-implementation's top neighbor often scores 0.5–0.7. Judge by reading the neighbor,
  not by the number.
- **Lexical mode** (`names`): a different scale entirely — 1.00 means "identical token
  set", which for `location` vs `location` is a duplicate but for `parse_header` vs
  `header_parse` is just word order. Treat it as a *prefilter*: it ranks what to read,
  it does not decide. Its blind spot is the mirror of the embeddings' strength — a
  re-implementation that shares no vocabulary scores 0.00.

## Language support

- **Rust** — tree-sitter: functions (with impl/trait/mod context, `///` docs), structs/enums/traits/type aliases.
- **TypeScript / JavaScript / TSX / JSX** — tree-sitter: function declarations, methods, arrow/function-expression bindings, interfaces/type aliases/enums/classes. `.d.ts` skipped.
- **Functor Lang** (`.fun`) — top-level `let`/`type` bindings with `//` docs; since Functor is
  file-= -module, top-level bindings are exactly the reuse surface.

Files listed in `.gitignore`, plus `node_modules`/`target`/`dist`/`build`/`vendor`, are skipped.
The `.dupfinder/` index directory gitignores itself.
