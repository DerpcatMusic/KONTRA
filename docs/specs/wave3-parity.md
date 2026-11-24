---
status: in-progress
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-24
updated: 2026-11-24
proof_scope: internal
evidence_freshness_days: 180
---

# Wave 3: Kontakt parity

Plan: `docs/architecture-v3/13-IR-VNEXT.md`, lane L4 ("translator laws with no
evidence → 0"). Stacked on Wave 2A.

## Audit (2026-11-24)

Sources: `tools/coverage/engine.tsv`, every `Impact::Audible` note in
`crates/format/kontakt/src`, the plan's open items. No Kontakt corpus or
native-oracle renders are mounted on the build host, so only gaps with
evidence already in the code or the Kontakt manual are closed here.

The coverage table was badly stale: its probes point at the retired v2
`sampler-kontakt/library.rs`, and it lists as `reported`/`absent` several
things the translator already runs (DBD, Glide, Multi LFOs, random-bipolar
and release-velocity sources, Group Dynamics).

## Acceptance criteria

1. **Start-condition precedence.** Kontakt compiles a group's start options
   into one unparenthesized KSP condition (`0140870000.c:5250-5281`), so
   `and`/`and not` bind tighter than `or`. `Behaviour::starts` evaluates the
   conditions as an `or` of `and` terms and still reads every atom, so each
   round robin steps. Regression test
   `engine/engine/tests/behaviour.rs::start_conditions_bind_and_tighter_than_or`
   fails on the old left fold.
2. **Truthful coverage rows.** The rows for DBD, Glide, Group Dynamics, Multi and
   Digital Multi LFOs, the random-bipolar and release-velocity sources, and the
   three start joins name current code. Their probes pass under
   `tools/coverage/build.py::probe_ok`.

## Non-goals

- Pass filters on the Solid G-EQ v0x11, Stereo Modeller pseudo stereo, the
  envelope follower, Glide routed to targets other than pitch, the
  `sequencer_only` start option, negative modulation lag, and the Tone, Beat,
  MP60 and S1200 machines. None of them has a native law in hand, so each
  stays a reported diagnostic until an oracle render or a binary read gives one.
- Rewriting the coverage rows not touched above (their probes still point at v2).

## Primary-Source & Version Proof

N/A — internal refactor: Kontakt behaviour is taken from in-repo binary references, the census of real libraries and existing oracle vectors; no external API or SDK assumption.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/wave3-parity.md` | Exit 0, zero proof errors | PASS (2026-11-24, linux-x86_64) |
| Fast unit/lib tests | Compile/ABI | linux-x86_64 | `matari fast kontra` | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | `matari check origin/main --project plugins/kontra` | fmt, dbg/todo guard, tests, clippy/ratchet green | PENDING |

## Unverified Boundaries & Promotion Gate

- Gaps kept as diagnostics (listed above) need Kontakt reference renders before they can be closed.
