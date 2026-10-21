# soothe3 pseudocode corpus

Every `.pdata` function in the restored soothe3 image, decompiled once with
radare2 + the Ghidra decompiler (`pdg`), indexed for search. Static analysis
only. The binary is never run.

Input: `/tmp/opencode/rea/soothe3-restored.bin` (sha256
`0143f03b…dbbcfa64`, image base `0x180000000`). Sweep index:
`/data/dev/workspace/graft-scratch/sweep-soothe3/index-default.sqlite`.

Heavy output is not in git. It lives under
`/data/dev/workspace/RunScratch/smooth/re-corpus/`:

- `corpus/fn/<rva>.c`: raw `pdg` output, one file per function (31,052 files).
- `corpus/manifest.tsv`: rva, end, size, section for every function.
- `index/corpus.sqlite`: FTS5 index plus call, string, constant, and vtable tables.
- `index/annotated/<rva>.c`: the same bodies with a header (owner, name hint,
  vtable, callers, strings, float constants) and `fcn.X` callees renamed to
  `Class::slotN_fcnX` when the callee is a vtable slot.

## Rebuild

```
tools/smooth-core/research/re-tools/run_corpus.sh            # resumable decompile + index
tools/smooth-core/research/re-tools/run_corpus.sh index      # index only, after a fix
```

- `corpus_build.py` takes the functions from `.pdata` (the exception directory,
  31,061 entries, 9 chained, so 31,052 starts). It writes one `r2 -i` script per
  batch of 120 functions and runs 8 batches in parallel. Each batch emits
  `=== BEGIN/END va ===` markers, and the parser splits them into per-function
  files. Batches are resumable through `batches/*.done`. A full run takes
  roughly 2–3 h when the machine is loaded (16 cores, other agents building).
  Batches that time out or crash Ghidra leave their functions missing. Re-run only
  those with `corpus_build.py --retry-missing --batch 4 …`. That mode never rewrites
  `manifest.tsv`. Fourteen functions still fail after retry (see the findings file).
- `corpus_index.py` joins the pseudocode with the sweep tables. It maps each
  site to its containing pdata range and extracts:
  - direct call and tail-jump edges (`call` table);
  - RIP-relative string references (`str_ref`);
  - SSE scalar float loads (`movss`/`addss` for f32, `movsd`/`addsd` for f64)
    from data, as `const_ref` (integer and pointer loads are not included);
  - RTTI class names (`vtable`, `vtable_slot`), demangled when simple and kept raw
    for templates and lambdas.
- `corpus_query.py` is the query CLI (below). Options go after the subcommand.

Environment: `SMOOTH_CORPUS_DB` selects the sqlite file (default under
RunScratch). `SMOOTH_CORPUS_RUN_DIR`, `SMOOTH_SWEEP_DB`, and
`SMOOTH_CORPUS_WORKERS` override the paths and worker count.

## Query

```
Q=tools/smooth-core/research/re-tools/corpus_query.py
$Q by-string 'soothe3' --limit 5
$Q by-const 0.7071 --tol 1e-4                 # 1/sqrt(2) as float32
$Q callers 0x1800f280                         # VA or RVA
$Q callees 0x180002300
$Q vtable Soothe3AudioProcessor --slot 2      # slot-by-slot, class substring
$Q fn 0x180001900                             # annotated pseudocode
$Q search 'spec AND 0x10' --limit 20          # FTS5 over names, bodies, strings, constants
$Q string-at 0x1f9af68
```

`search` uses FTS5 syntax (AND, OR, NOT, quoted phrases). Hex literals such
as `0x2c78` tokenize as-is. `fn_fts` is keyed by function RVA.

## Limits

- Ghidra `pdg` names every non-analyzed call target `func_0x…` or `fcn.<va>`.
  IPP entries have no symbols: they are recognized only by section (`IPPCODE`,
  `owner = IPP`) and by their code shape.
- Indirect calls (`call [reg+…]`) are not resolved into the call table. Only
  the sweep's direct edges are present, so `callers` misses calls through
  vtables. Use `vtable` to see slot targets instead.
- Float constants come from SSE scalar loads only. Packed constants (`mulps`
  on 16-byte tables) and constants built from integer immediates are not
  indexed.
- Pseudocode is `pdg` output without manual type recovery. Struct offsets appear
  as `*(int *)(param_1 + 0x2c78)`, so search by offset (`'0x2c78'`).
- RTTI names come from `.?AV…` type descriptors. They identify classes, not
  every method. Parameter identifiers are strings in `.rdata` (see
  `by-string`).
