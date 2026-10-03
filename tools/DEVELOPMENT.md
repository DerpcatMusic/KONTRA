# Short development loop

Use the existing incremental `dev` profile while editing. Keep optimized,
native-host, packaging, and complete-suite validation together at the final
checkpoint. This helper does not change shipping profiles or disable tests.

```sh
# Type-check only the affected target; no executable linking.
python3 tools/dev.py check
python3 tools/dev.py check --target cli
python3 tools/dev.py check --target standalone

# Import/DSP-only library edits can omit the plugin/UI feature graph.
python3 tools/dev.py check --core

# Build one test executable, then run exactly the requested tests.
python3 tools/dev.py test ui::tests::idle_editor_rebuilds_only_when_something_moves
python3 tools/dev.py test --target playback root_pitch_octave_reverse_and_end_bounds loop_crossfade_is_continuous_at_the_wrap

# Inspect a command without running Cargo or mutating a target directory.
python3 tools/dev.py --print check --target all
```

The default toolchain is `1.98.1`. Global options precede the command:
`--toolchain`, `--jobs` (default4), `--profile dev|dsp-dev`, and `--print`.
`dsp-dev` is the existing opt-level1 profile when a scoped DSP test needs it;
it is still development validation, not a release performance measurement.

`artifacts/dev-target` is persistent and separate from production/probe targets.
The helper deliberately ignores inherited `CARGO_TARGET_DIR` and enables
incremental compilation. To keep a development cache elsewhere, set
`KONTRA_DEV_TARGET_DIR`; assign separate directories to concurrent worktrees.
The first build of a target/profile still compiles its missing dependencies.

Tests always ask Cargo to verify current source/dependency inputs first. The
executable comes from that invocation's JSON artifact record, never a directory
mtime or a previously frozen release binary. Every requested name must match an
actual test exactly; names with no match fail. Ignored tests require an explicit
`--ignored`, and normal tests cannot accidentally be omitted by that flag.
Multiple names reuse the same compiled executable, running sequentially with
one test thread. No complete-suite or screenshot test is removed: request
`ui::tests::screenshot` explicitly when its images matter, and run the existing
full release/native gate batch before shipping.

Recent local checkpoint logs explain why these steps are separate: warm
all-target/all-feature `cargo check` completed in3.35–4.96s, release test linking
in105–125s, and production linking in58–134s. Full library runs took98–107s and
reported `ui::tests::screenshot` running over60s; the complete playback suite
took0.74s. These are historical wall times, not measurements of this helper or
a promised speedup on another machine.

## Reload boundary

Rust UI/DSP changes still need rebuilding the affected binary. The NativeUI
Lua runtime reevaluates cached component functions and KSP observations for
scene updates, but has no source-file watcher. Changing `.nui`/Lua resources
requires a fresh instrument import or script restoration, which prepares a
new scene and assets. Closing/reopening an editor is not a verified resource
reload. A live Rust/plugin or Lua file reload is not provided by this helper.

The helper itself can be checked without any Rust compilation:

```sh
python3 -m unittest discover -s tools -p test_dev.py
```
