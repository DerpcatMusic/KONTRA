# Assistant Regular host font

The explicitly requested Assistant / Regular font is supplied by the UVI host
resource provider only when it is absent from the program's own bank or loose
resources. Other families and styles are never substituted. A failed bank,
invalid path, ambiguous identity or read failure remains an error.

Upstream: https://github.com/hafontia-zz/Assistant
Commit: `bb678f0dcc89dedf193fe2cd362fd0a180e69883`
File: `Fonts/TTF/Assistant-Regular.ttf`
SFNT version: `Version 2.001`; family `Assistant`, style `Regular`, weight 400.
License: the adjacent upstream `OFL.txt`.

SHA256:
- Assistant-Regular.ttf: `45598982eee0844e6498178d6af35c84466d2e108570b93efbdc44c7ab014482` (75,500 bytes)
- OFL.txt: `ed4d2b16e40accaf530b5c27d8b96bdb7031a031b8c5314408d0945302d05ea4`

The byte slice is embedded; no font parsing or owned copy occurs until its
requested identity is read. UI font preparation uses the existing shared worker
and font parser. UVI's exact shipped font version and pixel-match are
**UNVERIFIED**. The belief that UVI supplies this font from its host app is an
inference, not an observed official-player resource read.
