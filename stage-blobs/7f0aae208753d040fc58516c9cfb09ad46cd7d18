# W12 requested-library family priority

The 1494-item manifest and enumerated Kontakt/UVI library roots contain no Chris Hein, ProjectSAM TrueStrike or Spitfire library. The relevant NI corpus entries are **Una Corda: 3 items/3 programs** and **Conflux: 51 items/101 programs**. Both appear in NI's [Komplete 15 product list](https://support.native-instruments.com/support/solutions/articles/69000879171-which-products-are-included-in-komplete-15-). This describes the local corpus, not the external tester's installed libraries or entitlement.

Source `5172a9ef` cached native metadata supplies 13,206 Una Corda zones and 101,265 Conflux zone records across those programs. Every priority program contains authored selection commands (`allow_group`, `disallow_group`, `ignore_event`, `play_note`, `set_event_par`, `set_event_par_arr`), so all **104 program classifications remain SCRIPT_DRIVEN / UNKNOWN**. Presence is a conservative guard, not proof that every command executes in every audition. Una Corda has one native cycle-position-2 criterion per program; Conflux has no static group-start criteria. Static metadata cannot certify their scripted family or RR distribution.

The gate's existing native-data result is unchanged: Pacific **3 MATCH cells**, **63 SCRIPT_DRIVEN cells UNKNOWN**. That is historical W12 execution evidence on its pinned source, not certification of the current release. No scanner, native host or v1 rerender was repeated to re-establish an already clear verdict.

Priority metadata losses, enabled/bypassed:

| Library path root | FX slots | Filter slots | Modulator slots |
|---|---:|---:|---:|
| `/mnt/MAIN_STORAGE/Libraries/Kontakt/Una Corda Library/` | 32/66 | 6/302 | 40/0 |
| `/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/` | 146/1 | 72/23 | 128402/0 |

Una Corda's direct target losses are `ahdsr_attack` 13/0, Filter6 `filterCutoff` 9/0, and EQv92 `eqGain2` and `eqGain3` 9/0 each. Conflux loses RandomBipolar 28050/0 and LFO6 9282/0 source slots; its largest direct target losses are `intensity` 64974/0 and `frequency` 37128/0. These source and route units remain separate. W15 received the target queue. Una Corda convolution `ResourceUnavailable` counts indicate failed resolution by our implementation, not proven absence of library resources.

First native-capture priority: `/mnt/MAIN_STORAGE/Libraries/Kontakt/Una Corda Library/Instruments/Una Corda Cotton.nki`, program 0, MIDI key 60, velocity 20. The historical reference reports a roughly 3,086-frame v2 start versus 8–22 native frames at this cell; it is a hypothesis for current attribution, not a current regression verdict. The historical 48-note grid has only four repeats per cell and cannot satisfy the new 32-repeat RR gate.

The prepared capture plan extends the historical keys **36/60/84**, velocities **20/60/100/127** to **32 repeats per cell** for all three Una Corda programs. It explicitly sets channel 0 and CC1=100, CC7=127, CC10=64, CC11=127, CC64=0; this is a new matched protocol. Native and v2 must share instrument/state identity and the complete MIDI schedule, sample rate and controller state. Compare attack and release source families, cursor offsets and directions independently; score RR support/distribution, not hit order. Identification failures remain UNKNOWN. Capture and actual-execution evidence must not use the v2 selector as the oracle.

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w12-fidelity/priority-komplete-native.json` (paths and numeric metadata) and `priority-native-capture-plan.json` (36 cells, minimum 32 repeats, asserted). No native application was launched, decrypted payload persisted, or WAV produced. Targeted independent-family and common-grouping checks PASS; the bounded-memory ranking self-check PASS. No new Rust implementation changed in this report.

NEXT: Una Corda Cotton start-offset/family attribution when independent captures are available; meanwhile recount W15's admission-changing EQ/source READY slices.
