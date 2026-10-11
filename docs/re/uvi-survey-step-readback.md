# StepEnvelope indexed readback (survey item 4)

AO V Strings Bartok reads `Level0` through `Level15` to populate its step controls.
The saved program stores the values together in the whitespace-separated `Levels`
attribute; comma decimal separators also occur in UVI XML. `NumSteps` selects the
running sequence length rather than the number of readable parameters.

Vendor references: [Element.getParameter](https://lua.uvi.net/class_element.html)
and [Elements / Step Envelope](https://lua.uvi.net/_elements.html), which documents
1–128 steps, default 16. The static web catalog omits the indexed parameters.
The local UVI Workstation User Guide describes 1–128 arpeggiator steps but does
not document the Script API indexed readback.

Binary evidence: UVIWorkstationx64.exe SHA256
`78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`, cached
Ghidra extraction `uvi-workstation/complete-export/pseudocode.c`, constructor
`0x141150e40`, lines 6548760–6549000. The constructor reserves 128 floats and
registers 128 indexed float descriptors with getter `0x141153110`. The initial
pattern is one at indices 0, 4, 8, 10, 12 and zero elsewhere. Saved levels replace
the corresponding entries. Readback uses native float precision.

Radare2 on the approved reference copy at `0x141151226` returned packed bytes,
so it does not independently verify the unpacked constructor. The cache's
unpacked text snapshot SHA256 is
`684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1`.
No native host execution or reference recording was performed. Validation uses
synthetic saved-value and constructor-default vectors, failing before the fix.

This change implements named readback only. Indexed parameter IDs, definitions,
and live writes remain outside this fix; no setter diagnostic is suppressed.
