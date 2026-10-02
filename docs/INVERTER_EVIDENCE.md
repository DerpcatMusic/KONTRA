# Native Group Inverter evidence

The [Kontakt effect reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference) documents two controls: Phase Invert negates signal polarity; Pan L <> R swaps the stereo channels. The [KSP engine parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters) exposes these as `ENGINE_PAR_PHASE_INVERT` and `ENGINE_PAR_LR_SWAP`. These operations can use the existing group stereo matrix once the serialized control mapping is established.

The native Inverter parser currently preserves exactly two boolean fields with neutral names `flag_0` and `flag_1`. Neither the local ni-file chunk taxonomy nor the upstream author's [Kontakt binary templates](https://github.com/monomadic/hexfiend-templates/tree/master/Kontakt) supplies named Inverter fields. Documentation of the two UI controls does not establish the order of the two serialized flags.

A bounded metadata probe linked against the exact production library from combined build `f4d7974` inspected the actual Una Corda Pure, Felt, and Cotton instruments. Each contains 209 Inverter slots, all version `0x60`:

| Instrument | Bypassed `[false,true]` | Active `[false,false]` | Active `[false,true]` |
| --- | ---: | ---: | ---: |
| Pure | 112 | 97 | 0 |
| Felt | 112 | 97 | 0 |
| Cotton | 112 | 96 | 1 |

Cotton's differing active slot is group 94 (`RESONANCE f`), slot 7, output gain 2.0. All three embedded script sets contain zero references to `ENGINE_PAR_PHASE_INVERT`, `ENGINE_PAR_LR_SWAP`, or `ENGINE_PAR_ENV_AHD`; they cannot correlate a named control write with a stored boolean. This is an actual playback omission, but assigning the active flag to either polarity inversion or channel swapping would guess an audible operation. The existing explicit unsupported warning and output gain behavior remain intact. No DSP parity test is claimed, because no audible mapping was changed.

The ignored `artifacts/combined-f4d7974/una-inverter-metadata-summary.json` and `una-inverter-metadata-probe.rs` retain the reproducible aggregate evidence without proprietary script or sample bytes. A paired native save before/after toggling each Inverter control, or an authoritative binary schema, is needed to establish the field mapping.

AHDSR mode flags are a separate omission. The [Kontakt modulation reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation) states that AHD-only is a complete one-shot attack/hold/decay envelope independent of key duration, and that disabling retrigger preserves the current envelope position while another note remains held. The vendor record preserves a neutral `unknown_flag` and opaque tail; it explicitly does not identify that flag as AHD-only or retrigger. The importer already warns that these switches are undecoded. They cannot be derived from the six decoded stage parameters or Inverter controls. No native group start-condition field mapping was established during this bounded investigation.
