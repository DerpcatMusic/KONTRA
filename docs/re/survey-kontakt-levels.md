# Survey 3962616c: level attribution

Compared with e9d918e9, the integrated survey measured Afflatus 2 Horns KS
at 0.214049831 -> 0.560724378 peak, and Morphology at
0.253639489 -> 0.199760884. These are patch/host measurements, not native
Kontakt reference recordings.

## Horns

A single paired CLAP measurement reproduced both Horns peaks exactly. Conditions:
48 kHz, 128-frame blocks, note 60/velocity 64, CC1=100, CC11=127, five
one-second notes with half-second releases, six-second peak window; identical
exported state and separate fresh host settings. The previous/current release
plugins were the frozen survey artifacts, not newly built binaries. Signal traces
were complete with zero dropped records. Timing was contended and instrumented;
no performance conclusion is drawn.

The first level divergence is the group peaking EQ. Saved active bands are
one-band v0x92 records with -6 dB gain; their live CC1 modulation changes gain
before rendering. Both builds report the same live gains (including +18).
For group 3/zone 67, EQ input peak is identical (0.009441256523132324), but
EQ output is 0.010685055516660213 -> 0.024662766605615616. Group 11/zone 242,
with live gain +18, changes EQ output 0.05318624526262283 -> 0.2610725462436676.
Slot mixer/output gains and the downstream chain do not explain the increase.
The active instrument chain includes Convolution and Send Levels; script init
sets convolution output gain to 0.03194415. That gain is common to the pair.

The measured EQ law introduced between these builds explains the direction and
magnitude. See `docs/architecture-v2/KONTAKT_DSP_LAWS.md`, EQ section, and
`crates/sampler-core/src/dsp/eq.rs`: displayed gain is scaled by
20*log10(2)/3, not ordinary dB. Normalized +1 (displayed +18) gives A=8 and
center response A^2=64 (+36.1236 dB). A patch-wide corrective gain would undo
this verified law and is not justified by the previous survey number.

Evidence: Kontakt 8 filter reference, EQ controls
<https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference>;
Oct-7 same-SHA Ghidra extraction `FUN_1409591c0`; radare2 confirmation at
0x140959302 (float exponent conversion) and constant 0x144f38bd0 (6.0f),
0x14470bad0 (0.5f, 1.0f). SHA256:
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Focused check `dsp::eq::tests::bell_matches_the_measured_kontakt_response` passed.
The trace attributes the regression report; it does not establish full native
patch audio parity. The integrated peak is retained pending a native recording.

## Morphology

The survey pair is the only before/after measurement. Source inspection finds
876 active Ladder records (saved v0x95 selector 30), plus selectors 33 and 41;
script init selects group LP4/LP1 and bus LP3. The newly implemented native
Ladder selector/output taps change this chain's response. See
`docs/re/kontakt-morphology.md`. No compensating patch gain is warranted from
an older build that did not implement those selectors. Unlike Horns, a paired
stage trace and native recording have not been obtained: the exact attribution
and native full-patch level remain open.
