"""Original source-module outgoing intensity base and frequency getters."""
import argparse,json,hashlib,struct,pathlib,math
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("metadata",type=pathlib.Path,help="hash-pinned Kontakt-engine pe.json")
parser.add_argument("output",type=pathlib.Path,help="numeric verification receipt")
args=parser.parse_args()
import pefile
from unicorn import Uc,UC_ARCH_X86,UC_MODE_64
from unicorn.x86_const import *
meta=json.loads(args.metadata.read_text())
binary=pathlib.Path(meta['path']);raw=binary.read_bytes()
assert meta['sha256'] == '0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8'
assert hashlib.sha256(raw).hexdigest() == meta['sha256']
pe=pefile.PE(data=raw,fast_load=True);base=pe.OPTIONAL_HEADER.ImageBase
cpu=Uc(UC_ARCH_X86,UC_MODE_64);cpu.mem_map(base,(pe.OPTIONAL_HEADER.SizeOfImage+4095)&~4095);cpu.mem_write(base,pe.get_memory_mapped_image())
obj,out,stack,stop=0x2000000,0x2100000,0x2200000,0x2300000
for a in [obj,out,stack,stop]:cpu.mem_map(a,0x10000)
cpu.mem_write(stop,b'\xcc')
def put(off,fmt,v):cpu.mem_write(obj+off,struct.pack('<'+fmt,v))
def f32(v): return struct.unpack('<f',struct.pack('<f',v))[0]
def invoke(entry, registers, n=500000):
 rsp=stack+0xff00-8;cpu.mem_write(rsp,struct.pack('<Q',stop))
 cpu.reg_write(UC_X86_REG_RSP,rsp);cpu.reg_write(UC_X86_REG_MXCSR,0x1f80)
 for reg,v in registers.items(): cpu.reg_write(reg,v)
 cpu.emu_start(entry,stop,timeout=1000000,count=n)
 assert cpu.reg_read(UC_X86_REG_RIP)==stop

rtti=json.loads((args.metadata.parent/'rtti.json').read_text())
type16=next(x for x in rtti if x['descriptor_rva']==0xa3b22c0)
vtable=base+type16['vtables'][0]['rva']
assert type16['vtables'][0]['contiguous_executable_entries'][21]=='0x140887a70'
group,context=0x3000000,0x3100000
for a in [group,context]:cpu.mem_map(a,0x10000)
cpu.mem_write(group+0x8e8,struct.pack('<Q',context))
physical_slots=[0,1,7,15]
patterns=[0,0x80000000,0x3e000000,0xbe800000,0x3f800000,0x7fc00000,0x7f800000]
checks=[]
for physical in physical_slots:
 source=obj+0x1000*physical
 cpu.mem_write(context+0xd74+16*physical,bytes([1]))
 cpu.mem_write(context+0xd68+16*physical,struct.pack('<Q',source))
 cpu.mem_write(source,struct.pack('<Q',vtable))
for pattern in patterns:
 for physical in physical_slots:
  source=obj+0x1000*physical
  for ordinal in range(16):
   # Distinct outgoing destinations; getter must use target ordinal and owning slot.
   bits=pattern if ordinal==0 else struct.unpack('<I',struct.pack('<f',physical/32.+ordinal/64.))[0]
   cpu.mem_write(source+0x140+ordinal*0x88+8,struct.pack('<I',bits))
 for physical in physical_slots:
  for ordinal in range(16):
   invoke(0x1408efd60,{UC_X86_REG_RCX:0,UC_X86_REG_RDX:group,UC_X86_REG_R8:0xe3+ordinal,UC_X86_REG_R9:physical})
   bits=cpu.reg_read(UC_X86_REG_XMM0)&0xffffffff
   expected=struct.unpack('<I',cpu.mem_read(obj+0x1000*physical+0x140+ordinal*0x88+8,4))[0]
   assert bits==expected,(physical,ordinal,hex(bits),hex(expected))
   checks.append(dict(physical=physical,ordinal=ordinal,bits=hex(bits)))
frequency=[]
for physical in physical_slots:
 source=obj+0x1000*physical
 for flag in [0,64]:
  for bits in patterns:
   cpu.mem_write(source+0xa4,struct.pack('<I',flag));cpu.mem_write(source+0xd0,struct.pack('<I',bits))
   invoke(0x1408efd60,{UC_X86_REG_RCX:0,UC_X86_REG_RDX:group,UC_X86_REG_R8:26,UC_X86_REG_R9:physical})
   got=cpu.reg_read(UC_X86_REG_XMM0)&0xffffffff
   expected=bits if flag else struct.unpack('<I',cpu.mem_read(0x14470bad0,4))[0]
   assert got==expected,(physical,flag,hex(got),hex(expected))
   frequency.append(dict(physical=physical,flag=flag,bits=hex(got)))
fallbacks=[]
for condition in ['inactive','null_source','null_context']:
 for physical in physical_slots:
  source=obj+0x1000*physical
  if condition=='inactive':cpu.mem_write(context+0xd74+physical*16,bytes([0]))
  if condition=='null_source':cpu.mem_write(context+0xd68+physical*16,struct.pack('<Q',0))
  if condition=='null_context':cpu.mem_write(group+0x8e8,struct.pack('<Q',0))
  for target in [0xe3,0xe3+15,26]:
   invoke(0x1408efd60,{UC_X86_REG_RCX:0,UC_X86_REG_RDX:group,UC_X86_REG_R8:target,UC_X86_REG_R9:physical})
   assert cpu.reg_read(UC_X86_REG_XMM0)&0xffffffff==0,(condition,physical,target)
   fallbacks.append(dict(condition=condition,physical=physical,target=target))
  cpu.mem_write(context+0xd74+physical*16,bytes([1]));cpu.mem_write(context+0xd68+physical*16,struct.pack('<Q',source));cpu.mem_write(group+0x8e8,struct.pack('<Q',context))
result=dict(binary_sha256=meta['sha256'],entry='0x1408efd60',target_getter='0x140887a70',vtable=hex(vtable),helper_stubs=[],intensity=checks,frequency=frequency,fallbacks=fallbacks,scope='saved outgoing-target base and saved normalized frequency getter only; no combination/live setter/host timing proof')
args.output.write_text(json.dumps(result,indent=2)+'\n');print('PASS original saved bases',len(checks),'frequency',len(frequency),'fallbacks',len(fallbacks))
