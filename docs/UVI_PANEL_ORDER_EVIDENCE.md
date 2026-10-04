# Authored Table styles and parent-subtree order

Reviewed 2026-10-04. These changes follow installed `817b03a` and remain source
UI/hosted-callback evidence. No native compositor or installed DAW comparison is
claimed.

UVI's [Table API](https://lua.uvi.net/class_table.html) documents `sliderColour`,
`drawInnerEdge` and `innerEdgeColour`. Each owned Clarinet bank authors five
solid-fill tables: three enable separators and two disable them. Budgeted owned
snapshots retain those settings, and the source renderer honors bar/separator
ink while retaining transparent backgrounds, theme fallback and bounded peak
reduction. Seventeen focused checks pass. Actual source gestures open both
Preferences and step panels, settle hosted callbacks and resize the same live
Table 720 → 360 → 1080 → 720. Both banks preserve bounds/styles and callback
indices, with zero rejected UI inputs. Native line width, default color and gloss
fidelity remain unverified.

A separate parent-order proof identifies a still-visible earlier-panel Knob
overlapping a later-panel Table. Flat constructor-order drawing lets that Knob
intercept the Table's middle cell. The owned snapshot now retains a separate
paint order grouping validated parent subtrees and authored sibling order, as
grounded in the [Panel API](https://lua.uvi.net/class_panel.html). Original widget
vectors/IDs remain unchanged. Current parents govern stale constructor lists;
missing current children append in stable constructor order. Raw reads, existing
sequence budgets, cycle/scope checks and iterative traversal retain the bounds.

Twenty-four focused checks pass, including the 4,096-depth tree and a genuine
overlap-interaction regression that fails under the old flat renderer. Both
actual banks open/resize the same live panels and select the previously blocked
curve cell 65, publishing 95.25 through its real changed callback; step cell 51
publishes 0.75. All four callbacks have zero rejected inputs. Owned paint vectors
contain each of the original/V2 273/285 widgets once. Geometry, visibility,
alpha, fonts, values, epoch admission and edit identities are preserved.

Twelve parent-order captures use source MUI with approved artwork and owned
fonts; representative narrow Preferences and bank-sized step views were reviewed.
They are not native screenshots. These proofs establish scoped authored
interactions, not native pixel fidelity, numerical unit calibration or complete
editor behavior.
