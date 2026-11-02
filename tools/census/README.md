# `tools/census` — the gap-inventory harness

The scripts that produced [`docs/re/gap-inventory-2026-11-01.md`](../../docs/re/gap-inventory-2026-11-01.md),
committed so the survey can be re-run instead of believed. They were written as
throwaway harness code for that pass and are kept here as harness code: they
read the census files, they do not live inside the engine, and nothing in the
build or the gate depends on them.

The **inputs** are your own Kontakt libraries plus a built workspace. The
**outputs** (4.2 GB) are deliberately not committed — `census.json`, the
extracted `scripts/`, the `.tsv`s and `compile-*.json` are all reproducible from
these files, and none of them is cheap to review.

## Prerequisites

| what | how |
|---|---|
| the two command-line tools | `cargo build --release -p kontra-inspect -p kontra-render` → `target/release/` |
| `dump_scripts` | **not on this branch**: the example lived in `crates/sampler-kontakt`, which the 2026-11-02 sync (`e80a9f22b`) removed. Step 3 cannot be re-run as written; the extracted `scripts/` from the 2026-11-01 pass are kept outside the repo with the rest of `$CENSUS_WORK` |
| `kspcheck`, the compile pass | `cargo build --release --manifest-path tools/census/kspcheck/Cargo.toml` (its own workspace, one dependency: `crates/script/ksp`) → `tools/census/kspcheck/target/release/kspcheck` |
| python 3 | standard library only; no packages |
| the sample library | the `.nki` files themselves. The 2026-11-01 pass read `~/Armory/VSTs/Kontakt1` (6,037 `.nki`). Nothing in this directory can stand in for them — steps 1, 2, 4, 7 and 8 need the files on disk; 3, 5, 6, 9 and 10 read only what the earlier steps wrote |

Every script takes its work directory from `CENSUS_WORK` (default
`/var/tmp/gaps`) and its binaries from `KONTRA_INSPECT`, `KONTRA_RENDER` and
`DUMP_SCRIPTS` (defaults: `target/release/kontra-inspect`,
`target/release/kontra-render`, `target/debug/examples/dump_scripts`, i.e.
relative to the repository root).

`LIBRARY_ROOT` (default `~/Armory/VSTs/Kontakt1`) is the corpus root the paths in
`census.json` are under. It decides which folder the tables call a *library*:
the directory above the nearest `Instruments` level below it, or — for a loose
tree with no `Instruments` level, like `#Non NICNT/<publisher>/…` — the first
directory under it. `library.py` holds that rule for `build_tsv.py` and
`ui_sample.py`, so a corpus elsewhere (`/mnt/lib/…`) only needs the variable
moved; nothing is read out of a fixed position in the path any more.

Keep one work directory for the whole run: the pass itself split things across
`/var/tmp/gaps` and `/var/tmp/keysweep`, and the step that wants that split is
called out below.

```bash
cd /path/to/kontra                       # the repository root
export CENSUS_WORK=/var/tmp/gaps         # 4.2 GB of space; not in the repo
```

## Command order

1. **Census** — one load and one render per `.nki`, six shards. Needs the
   library. This is the long step (6,037 files, each loaded and rendered at
   velocity 100 / hold 1.5 s; run the shards in parallel, `WORKERS=N` per shard).

   ```bash
   find "${LIBRARY_ROOT:-$HOME/Armory/VSTs/Kontakt1}" -name '*.nki' | sort > "$CENSUS_WORK/all_nki.txt"
   split -n l/6 -d "$CENSUS_WORK/all_nki.txt" "$CENSUS_WORK/list."
   for f in "$CENSUS_WORK"/list.*; do
     WORKERS=6 python3 tools/census/census_sweep.py "$f" "${f##*.}" "$CENSUS_WORK/partA" &
   done; wait
   python3 tools/census/merge_census.py "$CENSUS_WORK/partA/shard-*.jsonl" "$CENSUS_WORK/census.json"
   ```

   The merge is keyed on each record's `path`, last record wins, and a path
   keeps the position of its first appearance. `census_sweep.py` appends to its
   shard, so re-running a shard into the same directory leaves two records for
   every file it swept; the merge replaces them instead of doubling the corpus,
   and it prints how many repeats it replaced. That is what makes the documented
   "keep one work directory" workflow safe to re-run.

2. **Keysweep** — re-render the instruments that were silent at key 60 across
   nine keys (36…84), to separate "the harness picked a key this patch does not
   use" from "this patch does not sound". Needs the library; 704 instruments × 9
   renders.

   ```bash
   python3 tools/census/keysweep.py 4
   # -> "$CENSUS_WORK/keysweep-results.jsonl"
   ```

   (The pass kept this file as `/var/tmp/keysweep/results.jsonl`. Point
   `CENSUS_WORK` at a directory holding a copy or a symlink of it if you want to
   reproduce that run exactly.)

3. **Census analysis** — totals, refusals by cause, unanswered actions, no-zone
   and silent sets, per library (0.1 s). An instrument is counted as silent only
   when every one of the nine keys answered; a record that lost a render is
   counted as unmeasured instead, and `keys_answered` travels with each silent
   row. That is the rule the document's "9 of 9 keys answered" refers to.

   ```bash
   python3 tools/census/analyze_census.py        # -> census_summary.json
   ```

4. **Extract the scripts** — runs `dump_scripts` once per loaded instrument
   (3,694 instruments, 8,016 slots, 3.8 GB). Needs the library, because the
   script text lives inside the `.nki`.

   ```bash
   python3 tools/census/extract.py "$CENSUS_WORK/census.json" "$CENSUS_WORK" 8
   # -> manifest.jsonl, scripts/<instrument>/slot-<n>.ksp
   ```

5. **The compile pass** — `kspcheck` links `crates/script/ksp` and calls
   `kontra_ksp::compile` on every slot. Needs only the extracted scripts. 47 s
   for the whole corpus on this machine (release build).

   ```bash
   ls "$CENSUS_WORK"/scripts/*/slot-*.ksp | tools/census/kspcheck/target/release/kspcheck \
     > "$CENSUS_WORK/compile.tsv"
   python3 tools/census/compile_fail.py "$CENSUS_WORK/compile.tsv" "$CENSUS_WORK/compile_fail.json"
   ```

6. **The symbol pass** — the project's own scanner (`tools/re/ksp_symbols.py`)
   over the extracted scripts, eight shards, then merged. Needs only the
   extracted scripts; 63 s wall / 7.4 min CPU for the eight in parallel. The
   statuses come from the ledger in *this* checkout, so the bucketing follows the
   ledger as it stands, not as it stood on 2026-11-01.

   ```bash
   for k in 0 1 2 3 4 5 6 7; do python3 tools/census/analyze_symbols.py $k 8 & done; wait
   python3 tools/census/merge_symbols.py         # -> symbols.json
   ```

7. **The UI re-check** — 44 instruments sampled by library from those with
   unresolved assets, each re-loaded and each unresolved path tested against a
   lowercased index of the library's files (`Samples/` pruned). Needs the
   library.

   ```bash
   python3 tools/census/ui_sample.py 44 "$CENSUS_WORK/ui_sample.json"
   ```

8. **The full re-check** (optional; the diagnostics surface of every loaded
   instrument, one JSON line each). Needs the library.

   ```bash
   python3 tools/census/full_check.py "$CENSUS_WORK/census.json" "$CENSUS_WORK/full.jsonl" 8
   ```

9. **The table** — the ranked table of the inventory. Which cells are measured,
   exactly: the `instruments`, `libraries` and `references` columns, and every
   figure quoted inside a sentence, are computed from `census_summary.json`,
   `compile_fail.json`, `symbols.json` and `ui_sample.json`; `kind` and
   `what_it_would_take` are the author's judgement; `example_instruments` is a
   measured sample for the command and compile rows and a hand-picked example
   for the bookkeeping, reader, sound and UI rows. `gap-inventory.tsv` is
   committed beside the document, so give `build_tsv.py` its path to refresh it
   (the default writes into `$CENSUS_WORK`).

   ```bash
   python3 tools/census/build_tsv.py docs/re/gap-inventory-2026-11-01.tsv
   python3 tools/census/table_md.py docs/re/gap-inventory-2026-11-01.tsv   # the markdown table on stdout
   ```

10. **Two helpers for reading a failure.** `where.py` prints the source line for
    each error in a `compile.tsv`-shaped stream (`where.py compile.tsv
    num_slices_zone 10`), and `instruments.py` diffs two compile runs by
    instrument (`compile-before.tsv` / `compile-after2.tsv`) and prints how many
    instruments and how many slots went away.

`python3 tools/census/test_census.py` checks the harness itself: the library
root arithmetic on a corpus that is not at this machine's depth, silence with a
missing render, a shard merged twice, and a `compile.tsv` of absolute paths. It
needs none of the inputs above, and `tools/verify.sh` runs it.

## What a fresh checkout still cannot reproduce

- **The libraries.** 39 % of the corpus is measured from files that are not in
  this repository and cannot be. The counts are reproducible only on a machine
  with the same library set installed.
- **The brief's pre-aggregated table.** §6.1 compares this pass with a table
  handed over in the brief, which is not in the repository either; the
  comparison is recorded in the document, not re-derivable here.
- **The exact symbol bucketing of 2026-11-01.** Step 6 reads the live ledger, so
  running it today re-buckets against today's ledger (it did: `approximate`
  357 → 362, `unverified` 879 → 880, same 3,694 instruments).
- **Bit-identical renders.** Steps 1 and 2 record peaks and hashes from the
  renderer as it was on 2026-11-01. Re-running on a later build gives the same
  shape with the numbers of that build.

Everything else in the document — the census totals, the per-library rows, the
ranked table, the ordering populations — is regenerated by the commands above.
Verified on 2026-11-01 by re-running steps 3, 5, 6 and 9 against the pass's own
artifacts: `analyze_census.py` reproduced every count in `census_summary.json`
(the silence rows now also carry `keys_answered`, and the totals
`silent_unmeasured` and `keysweep_errors`, which the 2026-11-01 run did not
write), `compile_fail.py` reproduced `compile_fail.json` identically from
`compile.tsv`, and `build_tsv.py` reproduced the committed `gap-inventory.tsv`
byte-for-byte.
