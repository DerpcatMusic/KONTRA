# Global editor preferences

Settings contains an Interface scale row with 100%, 125%, 150%, 200%, and Reset. This zooms the complete UI using MUI's existing host driver and remains separate from the performance view's scale and physical display DPI. Scale changes update open editors in the same process. Old settings files default to 100%.

The last native window dimensions are saved in host logical points. New editor objects start at that global size; accepted host resize requests override it. A retained editor keeps its own geometry on reopening. Existing native resize requests and the resize corner continue to work, with the corner converting zoomed UI distances back to native logical dimensions. CLAP/VST3 host restoration has not been separately verified.

Production instances share the existing Settings snapshot and one stdlib worker. Edits publish in memory immediately; the worker coalesces pending notifications and saves after 250 ms of inactivity. Closing an editor requests a bounded flush. Stable frames neither clone/save settings nor enqueue changes. Tests keep isolated preference stores and never change the user's settings.

Focused checks cover cross-instance snapshot visibility, repeated geometry edits and restart persistence, default/invalid scale guards, root-change scan notification, and MUI zoom remaining independent of host DPI/resizing.
