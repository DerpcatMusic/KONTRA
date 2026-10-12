#!/usr/bin/env python3
"""Generate the public UI whitelist from compiler tables and the pinned spec inventory."""
import json, re, sys
from pathlib import Path
repo=Path(__file__).resolve().parents[2]
builtins=(repo/'crates/sampler-ksp/src/builtins.rs').read_text()
names=set()
# Compiler macro sections are the authority for supported UI/keyboard/state helpers.
section=''
for line in builtins.splitlines():
 if line.strip().startswith('// '):section=line.strip()[3:]
 if section in ['User interface.','Keyboard display.','Persistence.']:
  match=re.search(r'\w+\s+"([a-z_]+)"\s*\[',line)
  if match:names.add(match[1])
names.update(re.findall(r'"(\$CONTROL_PAR_\w+)"',builtins))
names.update(re.findall(r'\$CONTROL_PAR_[A-Z0-9_]+',(repo/'docs/architecture-v2/KSP_SYMBOLS.md').read_text()))
inventory=(repo/'docs/architecture-v2/KSP_SURFACE.json').read_text()
names.update(re.findall(r'"(\$CONTROL_PAR_\w+)"',inventory))
for chapter in json.loads(inventory)['chapters']:
 if chapter['service'] in ['script_ui','keyboard_state_ui']:
  for section in chapter['sections']:
   for identifiers in section['symbols'].values():names.update(identifiers)
# Opaque vendor extensions have no compiler table entry; keep only these public spellings.
names.update('$CONTROL_PAR_NKS_NUM_VALUES $CONTROL_PAR_NKS_STR_VALUES $CONTROL_PAR_NKS_STYLE $CONTROL_PAR_NKS_TYPE $CONTROL_PAR_X $CONTROL_PAR_Y'.split())
names.update(['make_persistent','make_instr_persistent','read_persistent_var','persistence_changed','ui_control','ui_controls','ui_update'])
text='\n'.join(sorted(names))+'\n';target=Path(__file__).with_name('ui-symbols.txt')
if '--check' in sys.argv:assert target.read_text()==text,'regenerate ui-symbols.txt'
else:target.write_text(text)
print(len(names),'public UI identifiers')
