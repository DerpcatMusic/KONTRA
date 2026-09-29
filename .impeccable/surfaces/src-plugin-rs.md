---
version: 1
slug: "src-plugin-rs"
primary_target: "src/plugin.rs"
related_targets: []
---

# Native sampler editor
Mode: Operate. The user explicitly requests a complete replacement closely resembling Kontakt and KODA, with intrinsic alignment and a much cleaner native layout. The previous giant pill rows and monospace panel are an anti-reference. Build code-led under the user's standing autonomous authorization.

## Direction contract
THESIS: A restrained sampler workstation: library navigation, a multi-instrument rack, performance controls and a keyboard. Familiar Kontakt/KODA organization is explicitly pinned by the user.
OWN-WORLD: Achromatic graphite surfaces, flat continuous panels, neutral selection accents, white sans-serif type, dense rectangular controls, square panel corners and thin rules. No invented artwork or decorative plots.
STORY: Choose a library, find an instrument, inspect its group, play it, and see its actual compatibility status.
FIRST VIEWPORT: An intrinsic toolbar; proportional artwork browser on the right with separate preset list; growing, scrollable multi-instrument rack with Mapping, Groups and Info views; stable black-and-white piano keyboard and status strip. Real names use wrapped intrinsic text; controls share baselines and consistent spacing. Selecting a library immediately filters the preset list without hiding the rack.
FORM: User-pinned Kontakt/KODA canon overrides seed db6c3c4d's assigned index 5. Alternative decorative worlds fail the user's explicit resemblance requirement. Code-led implementation.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## User correction
The user rejected the first redesign for excess rounding, nesting and gaps. The replacement must resemble KODA/Kontakt more closely: real library images on the right, dense flat rack and toolbar, minimal padding. Local library artwork loads in memory from wallpaper.png, NICNT or NKR; never invented or bundled. The audible bank is named independently of attempted import metadata, so failed imports cannot misidentify the sound.

## Expanded interaction contract
The user rejected the navy/gold single-bank workbench and requires a real multisampler. Sixteen independently loaded parts; stable identities across reorder; per-part MIDI port/channel, audio output, gain, pan, mute and solo; add, replace, duplicate and remove; native internal drag-and-drop; host state persistence. Native resizing uses fixed zoom so MUI reflows at the actual viewport. Artwork uses its source aspect ratio. Keys are neutral black/white and mouse press/release generates note on/off. Four input MIDI ports and eight stereo audio buses are exposed to the host; MIDI thru forwards supported channel events to one output. No claim of full Kontakt, KSP or Falcon parity. Native file-manager drops accept NKI files through MUI’s existing adapter and a small MOOSE editor callback. Standalone hardware-device selection remains a host function.

## Preset instrument view
Selecting a preset or rack name opens Instrument. The view resolves a literal KSP wallpaper resource name from the owner's local loose resources or NKR, preserves the image aspect in a bounded performance area, and offers connected manual level, pan, group selection and audition. A visible notice identifies scripted interfaces as unavailable. This is an incremental manual performance surface, not a reconstruction of proprietary controls or a KSP runtime. The compact rack summary and right library browser remain accessible.

The next increment resolves a bounded subset of KSP on-init code into a disabled preview of authored control positions, labels, menus and hidden panels. The wallpaper excludes the original 68px Kontakt header, which the host shell replaces. Preview widgets are static native approximations; saved variables, callbacks and custom widget skins remain unsupported. Manual audio controls sit above the preview. Full KSP behavior remains the objective, not a delivered claim.

Preset navigation shows NKI instruments and NKM multis in separate filters scoped to the selected library. The performance view navigates whole presets with Previous/Next and displays the library identity; raw group names are confined to the Groups editor. Clicking an existing instrument focuses it; dragging can add another instance. A multi expands embedded programs into named rack parts, subject to explicit missing-resource and routing/script limitations.
