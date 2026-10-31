
## How work is done here (standing rules)

**Rust.** Every agent writing, reviewing or porting Rust loads the `beautiful-rust` skill
first. It is vendored in this repository at `.claude/skills/beautiful-rust/` (`v3-review`
and `v3-task` sit beside it), so it is present in every checkout. The root `clippy.toml`
and the `[workspace.lints]` block in `Cargo.toml` are the project's clippy configuration.
Idiomatic is not a preference here: ownership over manual memory, `Result` over error
codes, enums over flag ints and magic strings, and tests that prove behaviour.

**Parity method.** Kontakt is reverse-engineered, not guessed: disassemble the binary
with Kuna or REA, have the agent read the pseudocode (or the disassembly where the
pseudocode is unclear), and measure against the running original where possible.
Every constant, threshold and law cites either a binary address or a measurement. A
value with no citation is not allowed in the tree.

**Architecture: write once in the IR.** A feature is implemented once — in the IR, the
engine or the DSP — in a form that knows nothing about any one format. Kontakt is an
*extension* that maps its own script commands, parameters, menus and DSP records onto
those shared representations, carrying the measured laws. UVI and Opus extensions map
onto the same representations. The test of a change: could another format's reader
reuse it unchanged? If the answer needs a `if format == Kontakt`, the logic belongs in
the extension, not the engine.

**Before searching, ask the graph.** `trail ask "<question>" --source`, `trail callers
<symbol>`, `trail skeleton <file>`, `trail grep "<literal>"`. Use it before grepping or
opening files.

**One clone, or commits go missing.** This machine has held three clones of the
repo at once (`kv3`, `kontra-src`, `kontra`) plus worktrees of each. A branch
created in one is invisible from another, which has already swallowed a lane's
commits until they were ferried across by hand. Before starting: `git worktree
list` and note the common dir. Create worktrees from the clone the rest of the
work is in, and if a lane must work elsewhere, push or fetch by path — do not
assume a branch exists because `git branch` in your clone shows it.
