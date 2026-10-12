# Requested Assistant Regular host font

Augmented Orchestra's retained scripts request Assistant Regular from two
script-relative artwork directories. The bank contains no member with that
basename. The complete Bartók host resolves all 533 declared images, while
these two font identities remain absent. The frozen scanner labels the result
`missing-images`, although all 85 requested image decodes pass and its 120
failed text styles are font failures.

The approved fix embeds the font author's unmodified OFL font in the UVI UI
resource provider. It matches only `Assistant-Regular.ttf` (case-insensitive,
family Assistant, style Regular); the requested directory may vary. Program
resources take precedence. Invalid paths, bank admission failures, ambiguous
members and failed reads never use a host resource in place of an error.
Other families, styles and asset extensions remain absent. No startup font
parsing or owned byte allocation occurs; the existing shared UI worker loads
and parses the font when requested.

[Upstream provenance and digests](../../crates/sampler-uvi/assets/assistant/README.md)
record commit `bb678f0dcc89dedf193fe2cd362fd0a180e69883`, version 2.001,
75,500 bytes, and the adjacent original OFL license. The exact version shipped
by UVI and **pixel-match against UVI are UNVERIFIED**. Host-app supply is an
inference, not an observed official-player resource read.

The scanner adapter now separates font and image failure identities and exposes
`missing_fonts` plus `missing_font_hashes`. Status precedence is budget hit,
paint error, blank, missing_font, missing-images, no-ui, original-ok. If both
font and image requests fail, `missing_font` takes precedence and the JSON
retains both counts. The shared exporter uses actual failed font requests, not a deficit between
declared style inventory and observed style success. `font_unresolved_styles`
retains that deficit separately: an inactive, unrequested font is unobserved,
not a failed service request. Older adapters without request failure counts
retain their reported category; their typography observation remains incomplete. Summary tables
and regression aggregates retain the new category.

The targeted witness set is Bartók, Coline MW, Diamond Crackling, Antartide,
Traveller, Alarm Rise and Desolate Braam. The last three cover the exceptional
158/160 failed-font-style rows rather than only the common 120-style case.
This is a sample of the reported 337-family gap, not a new full-corpus gate.
The first run loads and auditions all seven, with every requested lookup and
image decode successful. It exposes a scanner observation bug: a deficit of
63 declared styles whose font is not requested by the painter was classified
as font failure. Preparation correctly publishes only current page assets.
The adapter now unions observed successful font IDs and resource failures
across rendered pages and labels actual font-service failures independently
of unobserved inventory. Runtime font retention and resource-error behavior
are unchanged. The final shared-scanner rerun is **7/7 Original OK, loaded and audible**,
with all requested lookups/decodes successful and zero failed font requests.
Bartók records 90/90 lookups and 85/85 image decodes. Its retained inventory is
4,106 font styles with 4,043 observed successes; the other 63 are unrequested
styles, retained as unobserved inventory rather than fabricated failures.
The prior style-deficit classifier labeled all seven missing_font even after
all resource requests succeeded; the corrected service observation admits them.
No full 337/660 sweep or official-player pixel comparison was run.

The optimized shared-parser witness after normal UI initialization loads the
75,500-byte file with a 64 KiB RSS delta. Debug first-call deltas include debug
code/stack and parser startup and are not production memory acceptance. Exact
requested identity, other styles/families/extensions, loose-resource precedence
and failed-bank authority are covered by four passing resource tests. The
resource witness fails before supplying the font. Scanner taxonomy also has a
failing-first synthetic shared-driver check; real font failures retain their
category ahead of missing images, while paint errors retain precedence.

The complete declared-asset witness also passes: Bartók retains 10,145 widgets,
resolves all 533 declared images and every declared font. Clarinet retains
273 widgets, resolves 44 images and every declared font, and continues to
report its one genuinely absent inactive PNG. Both initialize without Lua
faults. This checks resource availability beyond the painter's current demand;
it is not pixel or gesture parity against the official player.
