# Convolution: IR preparation remains untraced

Status: implementation blocked at the [MCP evidence gate](kontakt-fx-2-evidence.md).

Docs first: Kontakt 8 [Effect Reference, Convolution](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference),
S.Rate, Preserve Length, Reverse, Auto Gain, Volume Envelope and Early/Late.

Historical binary leads: class CConvolver's reset `0x1419b5a80`,
filter allocation `0x1419b3a70`, filter removal `0x1419b3c80` and
filter-construction helper `0x1419b6d90`.
Allocation maps six filter selectors to internal indices 3,4,5,2,0,1.
That permutation does not establish cutoff curves or filter topology.

The general convolution law is y[n] = Σ h[k]x[n−k]. Native parity also
requires determining the prepared h: resampling, time scaling, early/late
split, filtering, envelope, reversal and gain normalization in the
native order. None of those laws follows from the generic convolution
identity or the already-decoded saved controls.

Confirmation: the cached CConvolver API was located and three functions
inspected. The Kontakt wrapper→IR preparation path is unresolved; no
native preparation vector or production test. No IR assets were changed.
