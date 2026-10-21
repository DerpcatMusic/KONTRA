"""Read-only original legacy cutoff wrapper-state probe; numeric evidence only.

Usage: legacy_cutoff_clock.py /path/to/Kontakt8.exe
Uses no-op base reset/audio callbacks and the ordinary branch with zero offset.
"""
import sys,struct,json,hashlib
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from kemu import Emu
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_XMM1
if len(sys.argv) != 2:
 raise SystemExit('Usage: legacy_cutoff_clock.py /path/to/Kontakt8.exe')
E=Emu(sys.argv[1],heap_mb=16)
stub=E.alloc(16);E.uc.mem_write(stub,b'\x31\xc0\xc3');vt=E.alloc(0x200)
for offset in [0x28,0x108,0x168,0x170]:E.write(vt+offset,'Q',stub)
E.write(vt+0x120,'Q',0x14095a5a0)
E.write(0x144fd4c54,'I',0)
calls=[]
def entered(uc,addr,size,opaque):
 raw=uc.reg_read(UC_X86_REG_XMM1).to_bytes(16,'little')
 calls.append(struct.unpack('<f',raw[:4])[0])
E.uc.hook_add(UC_HOOK_CODE,entered,begin=0x14095a5a0,end=0x14095a5a0)
rows=[]
for initial,goal in [(8.5,.5),(-8.5,.5),(.5,.3)]:
 obj=E.alloc(0x2000);E.write(obj,'Q',vt);E.write(obj+0x1e4,'f',-123.)
 E.call(0x1408de6e0,obj,f={1:48000.},limit=200000)
 E.write(obj+0x120,'I',128);E.write(obj+0x1a1,'B',1)
 E.write(obj+0x1dc,'f',initial);E.write(obj+0x1e0,'f',.25)
 calls.clear();E.call(0x140ae9f90,obj,0,0,0,0,limit=500000)
 before=dict(cutoff_clock=E.read(obj+0x1f0,'f'),setter_calls=list(calls))
 E.write(obj+0x1dc,'f',goal)
 calls.clear();E.call(0x140ae9f90,obj,0,0,0,0,limit=500000)
 rows.append(dict(initial=initial,goal=goal,before=before,after=dict(cutoff_clock=E.read(obj+0x1f0,'f'),setter_calls=list(calls))))
print(json.dumps(dict(native_sha256=hashlib.sha256(Path(sys.argv[1]).read_bytes()).hexdigest(),scope='LP1 prepare/real setter + original shared wrapper; global branch zero; offset zero; null modulation buffers; no-op audio/reset virtual calls; 48kHz,128frame calls,resonance .25; boundary phase uncalibrated in runtime',rows=rows),indent=2))
