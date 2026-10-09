"""Bounded original-instruction Multi waveform checks; no libraries or process launch."""
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
def run(phase,weights,width=.5,n=1,increment=0,normalize=False):
 cpu.mem_write(obj,bytes(0x200));put(0xb0,'d',phase);put(0xb8,'d',increment);put(0x100,'f',width)
 for i,w in enumerate(weights):put(0x128+4*i,'f',w)
 put(0x13c,'B',int(normalize))
 cpu.mem_write(out,b'\xff'*4*n);rsp=stack+0xff00-8;cpu.mem_write(rsp,struct.pack('<Q',stop))
 for r in [UC_X86_REG_RAX,UC_X86_REG_RBX,UC_X86_REG_RCX,UC_X86_REG_RDX,UC_X86_REG_R8,UC_X86_REG_R9,UC_X86_REG_RSI,UC_X86_REG_RDI,UC_X86_REG_RBP,UC_X86_REG_R12,UC_X86_REG_R14,UC_X86_REG_R15]:cpu.reg_write(r,0)
 cpu.reg_write(UC_X86_REG_RSP,rsp);cpu.reg_write(UC_X86_REG_RCX,obj);cpu.reg_write(UC_X86_REG_RDX,out);cpu.reg_write(UC_X86_REG_R8,n);cpu.reg_write(UC_X86_REG_MXCSR,0x1f80)
 cpu.emu_start(0x140b078d0,stop,timeout=1000000,count=500000)
 assert cpu.reg_read(UC_X86_REG_RIP)==stop
 return list(struct.unpack('<'+'f'*n,cpu.mem_read(out,4*n)))

phases = [i / 32 for i in range(33)]
cases = []
for normalize in [False, True]:
 for weight in [.3, 1., 2., -.7]:
  values = [run(p, [weight,0,0,0,0], normalize=normalize)[0] for p in phases]
  scale = weight / max(1., abs(weight)) if normalize else weight
  expected = [-scale*math.sin(p*math.tau) for p in phases]
  error = max(abs(a-b) for a,b in zip(values, expected))
  assert error < 2e-6, (weight, normalize, error)
  cases.append(dict(weight=weight, normalize=normalize, max_error=error, values=values))
blocks=[]
for n in [1,3,17,32,127,256]:
 phase=.125; increment=.0007
 values=run(phase,[1.,0,0,0,0],n=n,increment=increment,normalize=True)
 expected=[-math.sin((phase+i*increment)%1*math.tau) for i in range(n)]
 error=max(abs(a-b) for a,b in zip(values,expected))
 assert error<2e-6,(n,error)
 blocks.append(dict(frames=n,max_error=error))
result=dict(binary_sha256=meta['sha256'],entry='0x140b078d0',helper_stubs=[],scope='original digital waveform instructions only; no host timing, setter or live parameters claimed',cases=cases,blocks=blocks)
p=args.output
p.write_text(json.dumps(result,indent=2)+'\n')
print('PASS',len(cases)*len(phases)+sum(b['frames'] for b in blocks),'original-byte waveform values; max error',max(c['max_error'] for c in cases+blocks))
