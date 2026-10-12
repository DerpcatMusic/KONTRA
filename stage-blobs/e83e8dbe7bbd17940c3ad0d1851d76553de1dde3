"""Original normalized LFO frequency and physical-slot binding checks."""
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

def expected_increment(x, reciprocal):
 x=0. if math.isnan(x) or x<0. else min(x,1.)
 exponent=f32(f32(x*f32(14.316152572631836))-f32(6.584315776824951))
 fraction=f32(exponent-math.floor(exponent))
 correction=f32(f32(fraction-f32(fraction*fraction))*f32(.339709997177124))
 bits=int(f32(f32(f32(exponent+127.)-correction)*8388608.))
 hz=struct.unpack('<f',struct.pack('<I',bits))[0]
 return f32(hz*reciprocal)
# Existing phase/frequency lanes never overwrite each other's physical slots.
context,group,cache,event,lane=0x3000000,0x3100000,0x3200000,0x3300000,0x3400000
for a in [context,group,cache,event,lane]:cpu.mem_map(a,0x10000)
bindings=[]
for physical in [0,1,7,15]:
 for dense in [0,3]:
  for fresh in [False,True]:
   for a in [context,group,cache,event]:cpu.mem_write(a,bytes(0x10000))
   cpu.mem_write(obj,bytes(0x200));put(0xa4,'I',64)
   cpu.mem_write(group+0x590+8*dense,struct.pack('<Q',obj));cpu.mem_write(group+0x811+dense,bytes([physical]))
   cpu.mem_write(context+0x1138,struct.pack('<Q',cache));cpu.mem_write(event+4,struct.pack('<I',42))
   cpu.mem_write(cache+0x2818+(physical+0x1a0)*16,struct.pack('<I',42 if fresh else 41))
   cpu.mem_write(cache+(physical+0x421)*16,struct.pack('<Q',lane))
   rsp=stack+0xff00-8;cpu.mem_write(rsp+0x28,struct.pack('<I',dense))
   invoke(0x1408f1260,{UC_X86_REG_RCX:context,UC_X86_REG_RDX:0,UC_X86_REG_R8:group,UC_X86_REG_R9:event})
   assert struct.unpack('<Q',cpu.mem_read(obj+0xc8,8))[0]==(lane if fresh else 0)
   bindings.append(dict(physical=physical,dense=dense,fresh=fresh))
values=[float('-inf'),-1.,-0.,0.,1e-7,.01,.125,.25,.5,.75,.9,1.,2.,float('inf'),float('nan')]
values += [f32(i/1024) for i in range(1025)]
cases=[]
checkpoints=[]
for rate in [44100.,48000.,96000.]:
 reciprocal=f32(32./rate)
 for fragment in [1,3,17,32,127]:
  cpu.mem_write(obj,bytes(0x200));put(0xc0,'f',reciprocal);put(0x128,'f',1.);put(0x100,'f',.5)
  phase=0.;worst=0.
  for start in range(0,len(values),fragment):
   xs=values[start:start+fragment];n=len(xs)
   cpu.mem_write(lane,struct.pack('<'+'f'*n,*xs));put(0xc8,'Q',lane)
   expected=[]
   for x in xs:
    expected.append(-math.sin(phase*math.tau));inc=expected_increment(x,reciprocal);phase+=inc
    if phase>1.:phase-=1.
   invoke(0x140b078d0,{UC_X86_REG_RCX:obj,UC_X86_REG_RDX:out,UC_X86_REG_R8:n})
   got=struct.unpack('<'+'f'*n,cpu.mem_read(out,4*n))
   worst=max(worst,max(abs(a-b) for a,b in zip(got,expected)))
   assert cpu.mem_read(obj+0xb8,8)==struct.pack('<d',inc),(rate,fragment,start)
   if fragment==1 and start<15: checkpoints.append(dict(rate=rate,input_bits=struct.pack('<f',xs[0]).hex(),increment_bits=cpu.mem_read(obj+0xb8,8).hex()))
   assert cpu.mem_read(obj+0xb0,8)==struct.pack('<d',phase),(rate,fragment,start)
  assert worst<2e-6,(rate,fragment,worst)
  cases.append(dict(rate=rate,fragment=fragment,values=len(values),max_waveform_error=worst))
result=dict(binary_sha256=meta['sha256'],entries=['0x1408f1260','0x140b078d0'],helper_stubs=[],scope='physical-slot binding and normalized frequency conversion only; serialized getter, init/live law and host timing not claimed',binding_cases=bindings,waveform_cases=cases,checkpoints=checkpoints,checked_values=sum(c['values'] for c in cases))
args.output.write_text(json.dumps(result,indent=2)+'\n');print('PASS original frequency state/PCM',result['checked_values'],'binding cases',len(bindings))
