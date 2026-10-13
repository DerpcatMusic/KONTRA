"""Bounded Kontakt phase-form instruction vectors; no libraries or host launch.

Matches tools/dsp/verify_digital_lfo.py: hash-pinned PE mapping, owned inputs,
no substituted helpers. Native function and constants were inspected with r2.
"""
import argparse,pathlib
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("binary",type=pathlib.Path)
parser.add_argument("output",type=pathlib.Path)
import hashlib,struct,pefile,json,pathlib
from unicorn import Uc,UC_ARCH_X86,UC_MODE_64
from unicorn.x86_const import *
args=parser.parse_args();path=args.binary;raw=path.read_bytes();sha=hashlib.sha256(raw).hexdigest();assert sha=='0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8'
p=pefile.PE(data=raw,fast_load=True);base=p.OPTIONAL_HEADER.ImageBase;cpu=Uc(UC_ARCH_X86,UC_MODE_64);cpu.mem_map(base,(p.OPTIONAL_HEADER.SizeOfImage+4095)&~4095);cpu.mem_write(base,p.get_memory_mapped_image())
stack,stop=0x2200000,0x2300000
for a in [stack,stop]:cpu.mem_map(a,0x10000)
cpu.mem_write(stop,b'\xcc');rows=[]
for mode in [0,7,8,9,10,11,14,15,16,17,31,32,33]:
 for amount in [0.,.125,.5,.875,1.]:
  for phase in [0.,.125,.25,.5,.75,.875,.999999]:
   rsp=stack+0xff00-8;cpu.mem_write(rsp,struct.pack('<Q',stop));cpu.reg_write(UC_X86_REG_RSP,rsp);cpu.reg_write(UC_X86_REG_MXCSR,0x1f80)
   cpu.reg_write(UC_X86_REG_XMM0,struct.unpack('<I',struct.pack('<f',phase))[0]);cpu.reg_write(UC_X86_REG_XMM1,struct.unpack('<I',struct.pack('<f',amount))[0]);cpu.reg_write(UC_X86_REG_R8,mode)
   cpu.emu_start(0x140567aa0,stop,timeout=100000,count=10000);assert cpu.reg_read(UC_X86_REG_RIP)==stop
   val=struct.unpack('<f',struct.pack('<I',cpu.reg_read(UC_X86_REG_XMM0)&0xffffffff))[0];rows.append([mode,phase,amount,val])
args.output.write_text(json.dumps({'sha256':sha,'entry':'0x140567aa0','helper_substitutions':[],'cases':rows},indent=2)+'\n')
print('Native cases',len(rows),'helper substitutions: 0')
