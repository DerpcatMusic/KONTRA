---
version: 1
slug: "src-ui-inside-rs"
primary_target: "src/ui/inside.rs"
related_targets: ["src/ui/mapping.rs"]
---

## Direction contract
THESIS: Expose the instrument's sample mapping and overlapping alternatives as one readable workbench, with selection shared between Sound and the existing Mapping shortcut.
OWN-WORLD: Extend DESIGN.md's Graphite Rack: flat graphite planes, Noto Sans, square surfaces and hairlines. Functional group hues match map and list; selection stays neutral.
STORY: Fit or zoom/pan the key range, filter groups, point at a key/velocity cell, inspect its layers and RR takes, page through every overlapping take, select a sample and zoom/scroll its cached waveform and read its playback, loop-release exit and crossfade boundaries. Holding auditions through existing instrument routing. Source IR remains read only.
FIRST VIEWPORT: Sound tab strip above a compact group rail and key/velocity map; a compact navigation toolbar and paged overlap rows directly below, then selected sample properties and waveform with source-frame zoom/pan/Fit and start/end/loop/release-exit/crossfade markers. Two native plugin sizes: 1180×780 and 900×640.
FORM: Existing sampler workbench extension (incumbent Graphite Rack, no seed roll; seed key: incumbent-graphite-rack). Koda docs inform interaction only; no borrowed artwork or code.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
