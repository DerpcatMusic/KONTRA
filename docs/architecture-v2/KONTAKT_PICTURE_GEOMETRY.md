# Kontakt picture and text geometry

Kontakt picture dimensions and Label text/control dimensions are separate. NI
documents that the picture sidecar's Horizontal/Vertical Resizable flags permit
WIDTH/HEIGHT to resize the corresponding picture axis. The nonresizable axis
uses the known intrinsic frame dimensions. [NI Control Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters)

The shared painter previously preserved an explicitly sized Label's text bounds
but passed those same bounds to its background image. A nonresizable 150×69
picture therefore became 126×17, visibly squashing the artwork. This is distinct
from the wallpaper skin-offset defect fixed in `0e21446ff`.

`ir_view::label_background_size` applies intrinsic nonresizable axes only to KSP
and PerformanceView Label backgrounds with known picture dimensions. It leaves
text/control rectangles unchanged, uses authored dimensions on resizable axes,
and retains the existing nine-slice painter. The picture remains anchored at the
control's authored position even when smaller than the text box; the overlay's
default alignment would center a fixed-size child. Background contrast sampling uses
the same footprint. Missing intrinsic metadata keeps the existing dimensions;
Lua and other widget image policies are unchanged.

The dedicated synthetic pixel fixtures cover 100% and 200%, explicit selected
picture frames, larger pictures than text bounds, mixed-axis resizability,
nine-slice top/bottom edges, one-pixel backgrounds with independently rendered
text, and contrast sampling outside the text box. They contain no library
artwork or scripts. Real post-action library captures and a same-preset native
host comparison remain separate evidence; fixture success is not native parity.

Validation on 2026-10-15: the serialized shots-enabled UI cohort passed 411 tests
with 37 ignored and 388 filtered out, including all four picture geometry tests.
The first pixel run caught the smaller-picture centering defect; the assertions
were retained and the picture placement corrected. A new actual-library capture
after rebuilding the helper remains required to verify the reported fabric view.
