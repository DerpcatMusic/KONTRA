"""Bounded original target-expression emission; only CRT memory imports substituted."""
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

from unicorn import UC_HOOK_CODE
pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY['IMAGE_DIRECTORY_ENTRY_IMPORT']])
heap=0x4000000;cpu.mem_map(heap,0x1000000);bump=heap
imports={}
for directory in pe.DIRECTORY_ENTRY_IMPORT:
 for imp in directory.imports:
  if not imp.name:continue
  address=stop+0x100+len(imports)*8
  imports[address]=imp.name.decode();cpu.mem_write(imp.address,struct.pack('<Q',address));cpu.mem_write(address,b'\xc3')
substitutions=set()
def hook(cpu,addr,size,user):
 global bump
 if addr not in imports:return
 name=imports[addr];rcx=cpu.reg_read(UC_X86_REG_RCX);rdx=cpu.reg_read(UC_X86_REG_RDX);r8=cpu.reg_read(UC_X86_REG_R8)
 substitutions.add(name)
 if name=='malloc':
  assert rcx<0x100000;result=bump;bump+=(rcx+31)&~31;assert bump<heap+0x1000000
 elif name=='free':result=0
 elif name in ['memcpy','memmove']:
  assert r8<0x100000;cpu.mem_write(rcx,bytes(cpu.mem_read(rdx,r8)));result=rcx
 elif name=='memchr':
  assert r8<0x100000;found=bytes(cpu.mem_read(rcx,r8)).find(bytes([rdx&255]));result=rcx+found if found>=0 else 0
 elif name=='memset':
  assert r8<0x100000;cpu.mem_write(rcx,bytes([rdx&255])*r8);result=rcx
 else:raise RuntimeError('unsupported imported helper '+name)
 cpu.reg_write(UC_X86_REG_RAX,result)
cpu.hook_add(UC_HOOK_CODE,hook)
group,params,engine=0x3000000,0x3100000,0x3200000
for a in [group,params,engine]:cpu.mem_map(a,0x10000)
cpu.mem_write(group+8,struct.pack('<Q',engine));cpu.mem_write(group+0x9a0,struct.pack('<Q',params))
cases=[]
for flag in range(256):
 for mode in [0,1]:
  cpu.mem_write(params,bytes(0x1000));cpu.mem_write(params+8,struct.pack('<I',25));cpu.mem_write(params+0xd,bytes([flag]));cpu.mem_write(params+0x1a,struct.pack('<h',-1));cpu.mem_write(params+0x544,struct.pack('<I',9))
  rsp=stack+0xff00-8;cpu.mem_write(rsp+0x28,struct.pack('<Q',mode))
  try:invoke(0x140873040,{UC_X86_REG_RCX:group,UC_X86_REG_RDX:out,UC_X86_REG_R8:16,UC_X86_REG_R9:0},n=3000000)
  except Exception as e:
   print('FAIL',hex(cpu.reg_read(UC_X86_REG_RIP)),type(e).__name__,e,'helpers',sorted(substitutions));raise
  ptr,length,cap=struct.unpack('<QQQ',cpu.mem_read(out+8,24));ptr=struct.unpack('<Q',cpu.mem_read(out,8))[0] if cap>15 else out
  assert length<16384
  expression=bytes(cpu.mem_read(ptr,length)).decode()
  source='frange_uni(fcinv_neg($signal))' if mode else 'fcinv_flip($signal)'
  expected=f'1.0 - (1.0 - $in) * (1.0 - {source} * $intensity)' if flag&4 else f'($in * (1.0 - (1.0 - {source}) * $intensity))'
  actual=expression.split('$out := ',1)[1].split('end on',1)[0].replace('...','')
  assert ''.join(actual.split())==''.join(expected.split()),(flag,mode,actual,expected)
  cases.append(dict(flag=flag,source_bipolar=bool(mode),expression=expression))
result=dict(binary_sha256=meta['sha256'],entry='0x140873040',source='Constant native kind9; synthetic raw target ID25; no shaper or lag',helper_substitutions=sorted(substitutions),cases=cases,scope='original expression emission only; callback compiler/execution not tested')
args.output.write_text(json.dumps(result,indent=2)+'\n');print('PASS original emitted expressions',len(cases),'CRT substitutions',sorted(substitutions))
