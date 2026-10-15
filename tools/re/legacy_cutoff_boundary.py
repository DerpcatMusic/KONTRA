"""Read-only isolated legacy BFilter cutoff boundary oracle.

Usage: python3 tools/re/legacy_cutoff_boundary.py /path/to/Kontakt8.exe
Requires the existing kemu dependencies. Numeric metadata only on stdout.
"""
import sys,hashlib,json
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from kemu import Emu
if len(sys.argv) != 2:
 raise SystemExit('Usage: legacy_cutoff_boundary.py /path/to/Kontakt8.exe')
path=sys.argv[1]
e=Emu(path,heap_mb=32)
stub=e.alloc(16);e.uc.mem_write(stub,b'\xc3');vt=e.alloc(0x200);e.write(vt+0x28,'Q',stub)
classes=[(2,0x1408de6e0,0x14095a5a0),(3,0x1408de6e0,0x14095a200),(4,0x1408de730,0x14095a900),(6,0x1408de730,0x14095b2d0),(7,0x1408de730,0x14095ae80),(8,0x1408db380,0x14095c9c0),(9,0x1408db380,0x14095c410),(10,0x1408db380,0x14095b720),(11,0x1408db410,0x14095bdd0),(12,0x1408de790,0x14095cf60)]
rows=[]
for kind,prepare,setter in classes:
 for raw,bound in [(-8.5,0.),(8.5,1.),(9.7,1.),(10.,1.)]:
  images=[]
  for value in (raw,bound):
   obj=e.alloc(0x2000);e.write(obj,'Q',vt);e.write(obj+0x1e4,'f',-123.)
   e.call(prepare,obj,f={1:48000.},limit=200000)
   e.call(setter,obj,f={1:value,2:0.25},limit=200000)
   images.append(bytes(e.uc.mem_read(obj+0x200,0x1000)))
  equal=images[0]==images[1]
  rows.append(dict(kind=kind,raw=raw,boundary=bound,coefficient_state_equal=equal,coefficient_state_sha256=hashlib.sha256(images[0]).hexdigest()))
  assert equal,(kind,raw)
print(json.dumps(dict(native_sha256=hashlib.sha256(open(path,'rb').read()).hexdigest(),scope='isolated prepare and cutoff setter, 48kHz, resonance .25; fake no-op base reset; compare state +0x200..+0x1200; excludes stored raw cutoff +0x1e4; no wrapper clock or modulation ownership proof',rows=rows),indent=2))
