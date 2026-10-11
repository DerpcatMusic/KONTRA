# Legacy Phaser: four-stage cached topology

Status: native four-stage legacy Phaser implemented, free and tempo-synced Speed.

Docs first: Kontakt 8 [Effect Reference, Legacy Phaser](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference),
Depth, Speed, Phase, Feedback and Dry/Wet; KSP
[Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters), phaser controls.

Historical binary leads: BPar reader `0x140d01fa0` supports versions
0x50/0x51; audio `0x140aa3830`, initialization `0x1408ddb60`,
reset `0x14094c6d0`, routing `0x14095fde0`; shared mix preparation
`0x140ac28f0` and finalization `0x140ac21f0`.

The cached audio function uses four allpass stages per channel. Each
stage computes y = a(x + previous_y) − previous_x and advances its
two histories. Input feedback uses the preceding final-stage output.
An oscillator advances sine first by subtracting cosine·step, then
cosine by adding the new sine·step. Depth scales that oscillator before
the frequency mapping; the allpass coefficient is a rational function
of the mapped frequency. This is separate from saved filter type 13.

The saved field order is Depth, Feedback, Speed, Phase, unit. Reader
`FUN_140d01fa0` puts these at +8/+14/+c/+10/+30. Assignment
`FUN_1409619b0`, 0x140962c75–0x140962d4d, maps Depth directly,
Feedback to .85 times saved and Phase to .25 cycles times saved.
radare2 confirms the register loads and stores at those addresses.

Speed object vtable 0x144e74208 (radare2 dump) has index 5
`FUN_14072b330`, which calls `FUN_140a736a0`. Non-synced Speed is
already Hz. Unit > 9.999999717180685e-10 enables sync (0x144737708),
then speed is at least 1 and Hz=1/(((unit*60)/tempo)*speed), all f32,
clamped 0..8 Hz. Tempo below .1 BPM falls back to 120 BPM: radare2
`0x140a736ac..0x140a736b5` confirms the comparison and replacement,
with constants at `0x1447148d0` and `0x144f38cd0`. Unit's own +20 flag defaults to 1 in `FUN_140a723e0`;
it is distinct from the saved byte at BPar+21. The saved +3c/+40 values
and +21 byte do not participate in this getter. Bounds 0/8 are set in
`FUN_140701a80`. Runtime tempo reaches both voice and bus DSP contexts.

Oscillator step is f32(2*sin(pi*Hz/rate)). Reset puts left sine/cosine
at 0/1, and right at sin/cos(2*pi*(1-.25*Phase)); sine advances first.
The frequency polynomial has cutoff argument Depth*sine, factor 3,
square factor 9, then Taylor coefficients 1/2, 1/6, 1/24, 1/120, 1/720.
For abs(3*argument)<1e-5 the native branch uses 1. Otherwise the extracted
sum is evaluated with f32 intermediates. Mapped=(polynomial-1)*.0170626808;
a=(.9808333516-mapped)/(mapped+1.019166708). radare2 hexdumps confirm
0x144fc320c/0x144fc3d8c/0x144fc3da8 and polynomial constants at
0x144f38608/0x144f3864c/0x144f38694. Shared Mix retains physical dry,
wet, output and bypass ownership; the effect core itself emits wet only.

Evidence is the same-SHA complete-export Ghidra cache and radare2 on
`/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe`, SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Focused failing-first test
`effects::tests::legacy_phaser_free_speed_phase_and_four_stage_vector`
uses Depth=.5, Feedback=.25, Speed=1 Hz, Phase=1 at 48 kHz:
L=[.857850552,-.106721222,-.290372699,-.274628252],
R=[-.476720124,-.0511267483,.0421471819,.0595008172]
for input impulse [1,-.5]. Two beats at 120 BPM equals this free oscillator
bit-for-bit; synced tempo .05 BPM also equals 120 BPM. The low-tempo
assertion failed before adding the extracted fallback. These are independently computed f32 vectors, not host recordings.
