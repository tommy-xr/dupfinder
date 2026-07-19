# dupfinder

Duplication detection and reuse discovery for **Rust**, **TypeScript/JavaScript**, and
**[Functor Lang](https://github.com/tommy-xr/functor)** — built for LLM-assisted code review,
where the common failure mode is *re-implementing a helper that already exists, written just
differently enough that token-based clone detectors can't see it*.

Three engines in one CLI:

| Engine | Catches | Command |
| --- | --- | --- |
| **API index** | nothing — *prevents*: a greppable summary of every public function/type, so authors (human or LLM) can find the canonical helper before writing a new one | `dupfinder index` |
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

dupfinder review [DIR] [--base origin/main] [--top 3] [--min-lines 5]
    # review the current change: token clones touching the diff + the most
    # similar existing functions for each changed function. Base defaults to
    # origin/main / origin/master / main / master, first that exists.

dupfinder install-skill [--project] [--dir DIR]
    # install the bundled `review-for-duplicates` Claude Code skill.
    # default target ~/.claude/skills; --project writes ./.claude/skills;
    # --dir overrides the location. Re-run to update.
```

`review` is the CI/review-time entry point: its markdown output is designed to be handed
to a reviewer (human or LLM) as evidence — "this changed function closely resembles X;
should it reuse it?"

## Claude Code skill

dupfinder bundles a [Claude Code](https://claude.com/claude-code) skill,
`review-for-duplicates`, that drives `dupfinder review` and turns its output into a
judged, severity-ranked duplication review. Install it after `cargo install`:

```sh
dupfinder install-skill              # ~/.claude/skills (all projects)
dupfinder install-skill --project    # ./.claude/skills (this repo only)
```

The skill source lives in [`skills/review-for-duplicates/`](skills/review-for-duplicates/)
and is embedded in the binary, so `install-skill` always writes the version matching your
installed `dupfinder`.

## Reading similarity numbers

- **Pair mode** (`similar`): 0.90+ is a strong duplication signal.
- **Query mode** (`review`): scores are *rank*-meaningful, not absolute — a genuine
  re-implementation's top neighbor often scores 0.5–0.7. Judge by reading the neighbor,
  not by the number.

## Language support

- **Rust** — tree-sitter: functions (with impl/trait/mod context, `///` docs), structs/enums/traits/type aliases.
- **TypeScript / JavaScript / TSX / JSX** — tree-sitter: function declarations, methods, arrow/function-expression bindings, interfaces/type aliases/enums/classes. `.d.ts` skipped.
- **Functor Lang** (`.fun`) — top-level `let`/`type` bindings with `//` docs; since Functor is
  file-= -module, top-level bindings are exactly the reuse surface.

Files listed in `.gitignore`, plus `node_modules`/`target`/`dist`/`build`/`vendor`, are skipped.
The `.dupfinder/` index directory gitignores itself.
