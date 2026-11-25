---
status: draft
project: kontra
owner: agent (KSP language parity lane)
created: 2026-11-25
updated: 2026-11-25
proof_scope: internal
evidence_freshness_days: 180
---

# `detect_sample_type`, `detect_drum_type` and `detect_instrument_type`

## Problem & Goal

- **Problem:** KONTRA's three classifiers fail (`$NI_DETECT_*_TYPE_INVALID`, -1). The
  reference 8.13.1 reads 0 (instrument) for every signal measured so far
  (probes/ksp/detect_laws.kspp).
- **Goal:** the three rows of docs/PARITY.md (Kontakt KSP) read as the reference does.
- **Plan Reference:** the KSP parity lane of docs/architecture-v3/13-IR-VNEXT.md.

## Primary-Source & Version Proof

N/A — internal: the behaviour is decoded from the reference binary, written up with
addresses in docs/re/MIR.md (section "detectSampleType and the taggers").

## Status: blocked

**Blocked on a decision about shipping vendor model data.** Each classifier is a trained model:

- the sample type is an RBF SVM with Platt scaling (about 1 KB of model data);
- the drum and instrument taggers have 9 base64 models in the binary, about 1.4 MB decoded,
  including `InstrumentTaggingModel_0..5`.

Porting any of them means shipping the vendor's trained weights inside KONTRA. That decision is
held for the user. Until it is made, no weight data is extracted or committed. The decoded
locations stay in docs/re/MIR.md.

Estimate if it is approved: sample type about 250–350 lines (STFT, 64 mel bands, three feature
stages not yet decoded, SVM, Platt); drum and instrument taggers about 600–1000 more (a model
reader and their own feature stages, not yet decoded).

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/ksp-mir-classifiers.md` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Classifiers read as the reference | Validator/Runtime | linux-x86_64 | `matari fast kontra -p kontra-host -- -E 'test(detections)'` | 0 on the detect_laws signals | PENDING |

## Unverified Boundaries & Promotion Gate

- Every classifier law is unverified until a port is approved and measured against
  detect_laws.kspp.

## Non-goals and next steps

- No heuristic stand-in (always reading 0 would fake parity).
