# Compressor: alternate mode is a different detector

Status: implementation blocked at the [MCP evidence gate](kontakt-fx-2-evidence.md).

Docs first: Kontakt 8 [Effect Reference, Compressor](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference),
Classic/Enhanced/Pro modes and Stereo Link; KSP
[Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters).

Historical binary leads: selector `0x1409047a0` uses wrapper+0x1cc:
mode 0 calls `0x140ab2250`, mode 1 `0x140ab2c80`, mode 2
`0x140ab35d0`, then shared mixing.

Mode 1 uses the absolute linked signal, subtracts the threshold and
clamps negative excess to zero. Positive excess is scaled, transformed
through a piecewise linear table, passed through a two-state attack
recurrence and a decaying peak hold; the output gain is 1/(1+held).
With Stereo Link enabled the input to detection is the signed channel
mean, not the maximum of channel magnitudes. This cached interpretation
does not yet include all branch constants, parameter mappings or mode 2.

Confirmation: selector and mode-1 cached function inspected. Existing
mode-0 linking vectors in `tools/dsp-research/verify_laws.py` do not
confirm alternate modes. No alternate-mode vector or production test;
the current unknown-law diagnostic remains.
