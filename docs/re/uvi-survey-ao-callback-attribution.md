# AO V Strings Bartok callback attribution (survey item 2)

The survey's release CLAP build 3962616c measured 8,133.853 us median,
13,718.248 us p99, 2,250/2,250 missed deadlines at 48 kHz / 128 frames.
Its peak was 0.148311123. Machine load was reported around 20–30.

One targeted debug sound-core measurement on the same base, with this branch's
StepEnvelope readback and image-export fixes, measured:

| Quantity | Result |
|---|---:|
| Blocks | 2250 |
| Median render time | 737 us |
| p99 render time | 3731 us |
| Peak | 0.11016093 |
| End-of-run load averages (1/5/15 min) | 9.98 / 10.32 / 13.16 |
| Control polling total | 2.925 ms |
| Lua driver wake total | 626.004 ms |
| Stream service total | 15.396 ms |
| Runtime rendering total | 1880.166 ms |
| Empty render/service total | 6.557 ms |

Reproduction: ignored `sound::v2::tests::survey_ao_callback_attribution`.
Five C4 notes, velocity 64, 0.5 s gate once per second; CC1=100, CC11=127;
2250 blocks, a 2667-us sleep after each block. Loading occurs before timing.
The test uses the production V2Loader and threaded Lua driver. It does not run
the CLAP wrapper, atom refresh, GUI, or the host's scheduler; its timing and peak
are not a before/after host comparison. The slower run stretches Lua wall time
relative to the audio clock. Do not interpret the peak difference as a level fix.

The program translates to 6984 oscillator groups, nine buses, 401 routes,
and 14 modulators. Runtime voice rendering walks active voices, not all groups;
LFO evaluation occurs per active voice at control-cell boundaries. The raw
group count and missing LevelN reads therefore do not establish the survey's
sustained callback cost. The named LevelN getter now works, but no audio CPU
improvement is claimed from that change.

Remaining full-callback candidates include parameter/engine readback publication
in `src/plugin.rs::process`, including all effect descriptor refreshes whenever
the runtime control revision changes. Those lookups use binary searches, not
an established quadratic scan. They need a stack profile in the real host before
changing refresh semantics. The sound-core measurement did not reproduce the
all-block deadline failure; no speculative DSP optimization was applied.

The damaged bank's decode failures were not investigated or repaired.

## Round 2: full callback stack attribution

A fresh release build at `7f6bbfe9` with line tables and unstripped symbols
reproduces the sustained full-plugin callback problem. `perf record -e
cpu-clock:u --call-graph dwarf` and `task-clock:u` both fail to open events
with the machine's current profiling permissions (`perf_event_paranoid=2`);
noninteractive sudo also requires a password. No machine security setting was
changed. Fallback: launch the survey live-host under GDB, take argument-free
all-thread stacks every 25 ms, and continue after each SIGSTOP. This is
instrumented attribution, not a performance acceptance run.

Of 1,708 sampled stacks containing the CLAP/plugin callback, 1,676 (98.1%)
contain `V2Core::take_node_peaks`, chiefly its `nodes.iter().position` search.
`src/plugin.rs` publishes node meters every block. `Part::new` assigns runtime
bus b to tree node b+1, but publication searched all nodes for every bus: O(B*N).
AO's approximately 7,000 nodes therefore incur approximately 24 million
comparisons every callback, even while no voice sounds. This explains the
large cost absent from the previous sound-core render-only measurement.

The fix uses the constructor's existing bus+1 mapping and checks that the
node still names that bus before publication. It needs no new lookup table,
allocation, dependency, UI refresh throttling or DSP change. Every bus peak is
still taken/reset, unmapped buses remain omitted, and the group-meter fixture
also verifies resetting after publication. V1 `0cb7a8a0:src/engine/mod.rs` has
no corresponding nested node lookup to port; this uses v2's existing layout.
Uninstrumented paired timings and focused-test results follow below.
