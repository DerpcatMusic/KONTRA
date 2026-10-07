"""Bounded original-byte law checks, extending the read-only deep-dsp harness.
Usage: python verify_laws.py ARTIFACT_ROOT OUTPUT_JSON
No audio host is launched and no decoded library content is persisted.
"""
import importlib.util
import json
import math
import random
import struct
import sys
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *

ROOT = Path(sys.argv[1])
spec = importlib.util.spec_from_file_location('reference', ROOT / 'deep-dsp/verify_machine_code.py')
r = importlib.util.module_from_spec(spec)
spec.loader.exec_module(r)
CPU, OBJ, INPUT, CTX = r.CPU, r.OBJ, r.INPUT, r.CTX
f32, put, get = r.f32, r.put, r.get
EXTRA = {0x1443d111a: 'expf', 0x1443d10cc: 'exp', 0x1443d10b4: 'powf',
         0x1443d0ff4: 'memcpy', 0x1443d1000: 'memset', 0x140ab71b0: 'base_reset',
         0x1443c95d0: 'sincosf', 0x141aebb00: 'galois_size_topology', 0x1409591c0: 'eq_coefficients'}
CALLS = []


def xmm(cpu, reg, fmt='f'):
    return struct.unpack('<' + fmt, struct.pack('<Q', cpu.reg_read(reg) & ((1 << 64)-1))[:struct.calcsize(fmt)])[0]


def helper(cpu, address, size, unused):
    name = EXTRA.get(address)
    if name is None:
        return
    row = {'helper': name}
    if name in ('exp', 'expf', 'powf'):
        fmt = 'd' if name == 'exp' else 'f'
        args = [xmm(cpu, UC_X86_REG_XMM0, fmt)]
        if name == 'powf': args.append(xmm(cpu, UC_X86_REG_XMM1, fmt))
        result = math.pow(*args) if name == 'powf' else math.exp(args[0])
        row['arguments'] = args
        bits = int.from_bytes(struct.pack('<' + fmt, result), 'little')
        cpu.reg_write(UC_X86_REG_XMM0, bits)
    if name == 'sincosf':
        angle=xmm(cpu,UC_X86_REG_XMM0)
        row['angle']=angle
        cpu.reg_write(UC_X86_REG_XMM0,int.from_bytes(struct.pack('<ff',math.sin(angle),math.cos(angle)),'little'))
    if name == 'eq_coefficients':
        row['values'] = [xmm(cpu, reg) for reg in [UC_X86_REG_XMM1, UC_X86_REG_XMM2, UC_X86_REG_XMM3]]
    if name in ('memcpy', 'memset'):
        dest, source, length = [cpu.reg_read(reg) for reg in [UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_R8]]
        assert length<=0x40000
        cpu.mem_write(dest, bytes(cpu.mem_read(source,length)) if name=='memcpy' else bytes([source&255])*length)
        cpu.reg_write(UC_X86_REG_RAX,dest)
    CALLS.append(row)
    rsp = cpu.reg_read(UC_X86_REG_RSP)
    cpu.reg_write(UC_X86_REG_RIP, int.from_bytes(cpu.mem_read(rsp, 8), 'little'))
    cpu.reg_write(UC_X86_REG_RSP, rsp + 8)


CPU.hook_add(UC_HOOK_CODE, helper)


def call(entry, args=(), floats=(), fifth=None, rcx=OBJ):
    rsp = r.STACK + 0xff00 - 8
    CPU.mem_write(rsp, struct.pack('<Q', r.STOP))
    for reg in [UC_X86_REG_RAX, UC_X86_REG_RBX, UC_X86_REG_RSI, UC_X86_REG_RDI,
                UC_X86_REG_R10, UC_X86_REG_R11]: CPU.reg_write(reg, 0)
    for reg, value in zip([UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_R8, UC_X86_REG_R9], [rcx, *args, 0, 0, 0]): CPU.reg_write(reg, value)
    for i in range(16): CPU.reg_write(UC_X86_REG_XMM0 + i, 0)
    for argument in floats:
        slot, value, *fmt = argument
        CPU.reg_write(UC_X86_REG_XMM0 + slot, int.from_bytes(struct.pack('<'+(fmt[0] if fmt else 'f'), value), 'little'))
    if fifth is not None:
        value, fmt = fifth if isinstance(fifth, tuple) else (fifth, 'f')
        CPU.mem_write(rsp + 0x28, struct.pack('<'+fmt, value))
    CPU.reg_write(UC_X86_REG_MXCSR, 0x1f80)
    CPU.reg_write(UC_X86_REG_RSP, rsp)
    CALLS.clear(); r.STUB_CALLS.clear()
    CPU.emu_start(entry, r.STOP, timeout=2_000_000, count=2_000_000)
    assert CPU.reg_read(UC_X86_REG_RIP) == r.STOP, 'bounded call failed to return'


def clear(): CPU.mem_write(OBJ, bytes(0x80000))


def constants(addresses):
    return {hex(a): struct.unpack('<'+fmt, r.PE.get_data(a-r.BASE, struct.calcsize(fmt)))[0] for a,fmt in addresses}


def ahdsr_controls():
    rows = []
    for rate in [250.0, 1378.125, 1500.0, 3000.0, 6000.0]:
        for curve in [-1.0, -0.33, 0.0, 0.33, 1.0]:
            for x in [0.0, 0.25, 0.5, 0.75, 1.0]:
                clear(); put(0x14,'f',rate); put(0x120,'f',curve)
                for slot in range(5):
                    put(0x128 + slot*8,'Q',INPUT+slot*16)
                    put(slot*16,'f',x,INPUT)
                call(0x140ae46d0, [0])
                durations = [get(0xc4 + slot*4,'i') for slot in [0,1,2,4]]
                expected = []
                for scale in [8.922792434692383]*2 + [9.433565139770508]*2:
                    ms = f32(f32(math.exp(f32(f32(x*scale)+f32(math.log(2))))) - 2)
                    expected.append(int(f32(f32(ms*f32(0.001))*f32(rate))))
                assert durations == expected, (rate, curve, x, durations, expected)
                assert get(0x100,'f') == f32(f32(f32(x*x)*x)+f32(0.075))
                b = f32(math.exp(f32(1-abs(f32(curve)))*math.log(500000)-math.log(20000)))
                start = f32(b+1) if curve>0 else b
                ratio = f32(b/start) if curve>0 else f32(f32(b+1)/b)
                assert get(0xf4,'f') == start and get(0x11c,'f') == ratio, (curve, start, ratio, get(0xf4,'f'), get(0x11c,'f'), CALLS)
                for offset, n, base in [(0xdc,durations[0],ratio),(0xe4,durations[2],3/43),(0xec,durations[3],3/43)]:
                    if n: assert abs(get(offset,'f') - f32(base**(1/n))) < 2e-7
                rows.append({'rate':rate,'curve':curve,'normalized':x,'control_ticks':durations,
                             'attack_start':start,'attack_ratio':ratio,'sustain_with_floor':get(0x100,'f'),
                             'multipliers':[get(i,'f') for i in [0xdc,0xe4,0xec]],'math_calls':CALLS.copy()})
    return rows


def ahdsr_kernel():
    rows=[]
    rng=random.Random(8131)
    for stage in range(6):
        for frames in [0,1,3,4,5,31,32,33,129]:
            clear(); value=f32(rng.uniform(0.1,1.1)); multiplier=f32(rng.uniform(0.99,1.01))
            scale=f32(rng.uniform(-1,1)); bias=f32(rng.uniform(0,1))
            for offset,fmt,v in [(0xc0,'i',stage),(0xbc,'i',1000),(0x10c,'i',1000),(0xb8,'f',value),
                                 (0x110,'f',scale),(0x114,'f',bias),(0xdc+stage*4,'f',multiplier)]: put(offset,fmt,v)
            CPU.mem_write(INPUT,b'\x55'*(4*(frames+1)))
            call(0x140ae40f0,[INPUT,frames])
            expected=[]; state=value
            for _ in range(frames):
                expected.append(f32(f32(f32(state-f32(0.075))*scale)+bias));state=f32(state*multiplier)
            assert bytes(CPU.mem_read(INPUT,frames*4)) == struct.pack('<'+'f'*frames,*expected)
            assert get(0xb8,'f')==state and get(0xbc,'i')==1000-frames and get(0x10c,'i')==1000-frames
            assert bytes(CPU.mem_read(INPUT+frames*4,4))==b'\x55'*4
            rows.append({'stage':stage,'ticks':frames,'initial':value,'multiplier':multiplier,'scale':scale,'bias':bias,'final':state})
    return rows


def galois_parameters():
    rows=[]
    for index in range(10):
        for x in [0.0,0.25,0.5,0.75,1.0]:
            clear(); put(0x6dc8,'Q',INPUT)
            call(0x141aea370,[index],[(2,x)])
            mapped=get(0x6cd8+index*4,'f')
            # Coefficient/topology storage, not UI units.
            expected = {0:lambda:f32(f32(math.exp(f32(x*f32(3.7))))*0.5),
                        1:lambda:f32(f32(x*3)+1),2:lambda:f32(x**0.25),
                        3:lambda:f32(x*10),4:lambda:f32(x*0.5),5:lambda:f32(x*11025),
                        6:lambda:f32(f32(f32(1-x)*19000)+2000),7:lambda:f32(x*-12),
                        8:lambda:x,9:lambda:x}[index]()
            assert mapped==expected, (index,x,mapped,expected,CALLS)
            rows.append({'index':index,'x':x,'internal':mapped,'math_calls':CALLS.copy()})
    return rows


def eq_saved_conversion():
    rows=[]
    for frequency in [20,50,100,440,1000,5000,10000,20000]:
        for bandwidth in [1/3,1.0,2.0,3.0]:
            for gain in [-18.0,-6.0,0.0,6.0,18.0]:
                clear();call(0x1409590a0,[0],[(2,frequency),(3,bandwidth)],gain)
                values=CALLS[0]['values'];assert len(CALLS)==1
                f=f32(min(1,max(0,f32(f32(f32(frequency)-20)*f32(5.0050050049321726e-5)))))
                t=f32(f32(f*468.7109375)+1)
                z=f32(f32(struct.unpack('<i',struct.pack('<f',t))[0]*2**-23)-127)
                frac=f32(z-math.floor(z))
                expected=f32(f32(z+f32(f32(frac-f32(frac*frac))*f32(.346607)))*f32(.11257956176996231))
                assert values[0]==expected,(frequency,values[0],expected)
                assert values[1] == f32(f32(f32(bandwidth)-f32(1/3))*f32(0.375))
                assert abs(values[2]-f32(gain/18))<2e-7
                rows.append({'hz':frequency,'octaves':bandwidth,'db':gain,'normalized':values})
    return rows


def eq_coefficients():
    rows=[]
    del EXTRA[0x1409591c0]
    bitfloat=lambda bits:struct.unpack('<f',struct.pack('<i',bits))[0]
    def exp2approx(z):
        z=f32(z);frac=f32(z-math.floor(z))
        return bitfloat(int(f32(f32(f32(z+127)-f32(f32(frac-f32(frac*frac))*f32(.33971)))*8388608)))
    for rate in [8000,48000,192000]:
        for cutoff in [0,.25,.5,.75,1]:
            for width in [0,.5,1]:
                for gain in [-1,-.5,0,.5,1]:
                    clear();put(8,'f',rate)
                    call(0x1409591c0,[],[(1,cutoff),(2,width),(3,gain)],(7,'B'))
                    u=f32(f32(exp2approx(f32(cutoff*f32(8.882606506347656)))-1)*f32(.0021335112396627665))
                    hz=f32(f32(min(1,max(0,u))*19980)+20)
                    omega=f32(min(f32(hz/rate),f32(.49))*2*math.pi)
                    sine,cosine=f32(math.sin(omega)),f32(math.cos(omega))
                    A=exp2approx(f32(f32(f32(f32(gain*.5)+.5)*6)-3))
                    B=2**float(f32(f32(width*f32(2.6666667461395264))+f32(1/3)))
                    alpha=sine*(.5/(1/(B-1/B)))
                    p1,q=alpha/A,alpha*A;d=1/(1+p1)
                    expected=[f32((q+1)*d),f32(-2*cosine*d),f32((1-q)*d),f32(2*cosine*d),f32((p1-1)*d)]
                    actual=[get(o,'f') for o in [0x138,0x128,0x12c,0x130,0x134]]
                    assert actual==expected,(rate,cutoff,width,gain,actual,expected,CALLS)
                    rows.append({'rate':rate,'cutoff':cutoff,'width':width,'gain':gain,'frequency':hz,'coefficients':actual})
    EXTRA[0x1409591c0]='eq_coefficients'
    return rows


def wrapper_cadence():
    rows=[]
    modules=[('Replika',0x1408f98d0,53,0x14580,0x145f0),
             ('Reverb',0x1408fc160,11,0xe480,0xe498),
             ('SolidEQ',0x1408fbd60,16,0x14060,0x14080),
             ('SolidBusComp',0x1408fbb60,11,0x29a8,0x29c0)]
    # Fixed direct calls use the same observations as virtual core calls.
    r.STUBS.update({0x141ac70f0:'core_parameter',0x141ab4d70:'core_update',0x141aaa620:'core_process',0x141ac69e0:'core_parameter',0x141aaa1e0:'core_process',0x1404f99a0:'core_update'})
    for name,entry,count,flags,pointers in modules:
        for initial,blocks,enabled in [(0,[0,1,31,1,7,24,63],True),(5,[7,65],True),(32,[33,1024],True),(0,[65],False)]:
            clear();put(0x1bc,'i',initial);put(0x124,'I',2)
            vt=INPUT+0x30000;put(0x1c0,'Q',vt)
            for slot,address in [(0x60,r.STOP+0x100),(0x40,r.STOP+0x200),(0x38,r.STOP+0x300)]:put(slot,'Q',address,vt)
            for c in range(2):put(0x20+c*8,'Q',INPUT+c*0x1000)
            values=[[-.5,.25,.75,1.5][i%4] for i in range(count)]
            for i,value in enumerate(values):
                pointer=INPUT+0x10000+i*0x100
                put(flags+i,'B',enabled);put(pointers+i*8,'Q',pointer)
                CPU.mem_write(pointer,struct.pack('<40f',*([value]*40)))
            countdown=initial
            for frames in blocks:
                put(0x120,'I',frames);call(entry,[CTX])
                expected=[];offset=0
                while offset<frames:
                    if countdown<1:
                        countdown=32
                        if enabled:expected.extend([('core_parameter',i,min(1,max(0,x))) for i,x in enumerate(values)])
                        expected.append(('core_update',))
                    n=min(countdown,frames-offset);expected.append(('core_process',n,offset));offset+=n;countdown-=n
                actual=[]
                for c in r.STUB_CALLS:
                    if c['helper']=='core_parameter':actual.append(('core_parameter',c['index'],c['value']))
                    if c['helper']=='core_update':actual.append(('core_update',))
                    if c['helper']=='core_process':actual.append(('core_process',c['frames'],(c['inputs'][0]-INPUT)//4))
                assert actual==expected and get(0x1bc,'i')==countdown,(name,initial,frames,actual,expected)
                rows.append({'module':name,'initial':initial,'frames':frames,'enabled':enabled,'events':actual,'final':countdown})
    return rows


def stereo():
    rows=[]
    for rate in [8000,44100,48000,96000,192000]:
        for width in [0,.25,.5,.75,1]:
            for pseudo in [False,True]:
                if not pseudo and rate!=48000:continue
                for pan in [-1,-.25,0,.25,1]:
                    clear();n=1024
                    for off,fmt,v in [(0x14,'f',rate),(0x120,'I',n),(0x124,'I',2),(0x128,'I',2),
                                      (0x1c0,'f',width),(0x1c4,'f',width),(0x1d8,'f',pan),(0x1e0,'f',pan),(0x1dc,'B',pseudo)]:put(off,fmt,v)
                    samples=[[f32((i%13-6)*.0625) for i in range(n)],[f32((i%17-8)*.03125) for i in range(n)]]
                    for c in range(2):
                        put(0x20+c*8,'Q',INPUT+c*0x2000)
                        CPU.mem_write(INPUT+c*0x2000,struct.pack('<'+'f'*n,*samples[c]))
                    call(0x140aa5ac0,[CTX])
                    skip=r.STUB_CALLS[-1]['fifth_argument_byte']
                    delay=min(1023,int(f32(f32(f32(f32(rate*f32(.01))*width)*width)*width))) if pseudo else 0
                    if not skip:
                        out=[[],[]]
                        for i,(l,right) in enumerate(zip(*samples)):
                            if pseudo:lout,rout=l,samples[1][i-delay] if i>=delay else 0
                            elif width>=.5:
                                spread=f32(f32(f32(width-.5)+width)-.5)
                                lout=f32(f32(l+f32(spread*l))-f32(spread*right))
                                rout=f32(f32(right+f32(spread*right))-f32(spread*l))
                            else:
                                a=f32(.5-width);lout=f32(l+f32(a*f32(right-l)));rout=f32(right+f32(a*f32(l-right)))
                            out[0].append(f32(lout*f32(1-max(0,pan))))
                            out[1].append(f32(rout*f32(1+min(0,pan))))
                        for c in range(2):
                            actual=struct.unpack('<'+'f'*n,CPU.mem_read(CTX+0x9c010+c*0x1000,n*4))
                            assert all(abs(a-b)<2e-7 for a,b in zip(actual,out[c])),(rate,width,pseudo,pan,c,actual[:4],out[c][:4])
                    put(0x1e4,'f',1);put(0x11e4,'i',89);put(0x1c4,'f',.123);put(0x1e0,'f',.456)
                    call(0x14094cd90)
                    assert get(0x11e4,'i')==0 and get(0x1c4,'f')==width and get(0x1e0,'f')==pan
                    assert bytes(CPU.mem_read(OBJ+0x1e4,0x1000))==bytes(0x1000)
                    rows.append({'rate':rate,'width':width,'pan':pan,'pseudo':pseudo,'right_delay':delay,'unity_skip':bool(skip),'reset_checked':True})
    return rows


def stereo_smoothing():
    rows=[];kw=f32(1/180);kp=f32(1/1800)
    for initial,target in [(.6,.9),(.9,.6),(.1,.4),(.4,.1)]:
        for pi,pt in [(-.8,-.2),(-.2,.8),(.8,.2)]:
            for n in [1,3,4,5,31,32,33,1024]:
                clear()
                for off,fmt,v in [(0x120,'I',n),(0x124,'I',2),(0x128,'I',2),(0x1c0,'f',target),(0x1c4,'f',initial),
                                  (0x1d8,'f',pt),(0x1e0,'f',pi)]:put(off,fmt,v)
                for c,v in enumerate([.25,-.125]):
                    put(0x20+c*8,'Q',INPUT+c*0x2000);CPU.mem_write(INPUT+c*0x2000,struct.pack('<'+'f'*n,*([v]*n)))
                call(0x140aa5ac0,[CTX]);u=f32(initial);p=f32(pi);ps=[];us=[]
                for _ in range(n):
                    us.append(u);u=f32(u+f32(f32(f32(target)-u)*kw))
                for _ in range(n//4):
                    ps.append(p);p1=f32(p+f32(f32(f32(pt)-p)*kp));ps.append(p1)
                    p2=f32(p1+f32(f32(f32(pt)-p1)*kp));ps.append(p2)
                    delta=f32(f32(f32(pt)-p2)*kp);p3=f32(p2+delta);ps.append(p3);p=f32(p3+delta)
                for _ in range(n%4):
                    ps.append(p);p=f32(p+f32(f32(f32(pt)-p)*kp))
                assert get(0x1c4,'f')==u and get(0x1e0,'f')==p,(initial,target,pi,pt,n,get(0x1c4,'f'),u,get(0x1e0,'f'),p)
                outputs=[[],[]]
                for w,pn in zip(us,ps):
                    if initial>=.5:
                        spread=f32(f32(f32(w-.5)+w)-.5)
                        a=f32(f32(f32(spread+1)*.25)-f32(spread*-.125))
                        b=f32(f32(f32(spread+1)*-.125)-f32(spread*.25))
                    else:
                        blend=f32(.5-w);a=f32(f32(f32(1-blend)*.25)+f32(blend*-.125));b=f32(f32(blend*.25)+f32(f32(1-blend)*-.125))
                    outputs[0].append(f32(a*f32(1-max(0,min(1,pn)))))
                    outputs[1].append(f32(b*f32(min(1,max(0,f32(1+pn))))))
                for c in range(2):
                    actual=struct.unpack('<'+'f'*n,CPU.mem_read(CTX+0x9c010+c*0x1000,n*4))
                    assert actual==tuple(outputs[c]),(initial,target,pi,pt,n,c,actual[:4],outputs[c][:4])
                rows.append({'width_initial':initial,'width_target':target,'pan_initial':pi,'pan_target':pt,'frames':n,'final_width':u,'final_pan':p})
    return rows


def bus_timing():
    rows=[];setter=r.STUBS.pop(0x141ac69e0,None)
    for rate in [8000,44100,48000,96000,192000]:
        for index in [2,3]:
            for x in [0,.099,.1,.101,.25,.4,.5,.75,.899,.9,.901,1]:
                clear();desc=0x18+index*0x240;put(desc+8,'d',0);put(desc+16,'d',5)
                for channel in range(16):put(0x1aa0+channel*0xc0,'d',rate)
                call(0x141ac69e0,[index],[(2,x)])
                enum=math.floor(float(f32(x))*5+.5)
                table=0x1479a7e78 if index==2 else 0x1479a7ea8
                t=struct.unpack('<d',r.PE.get_data(table-r.BASE+enum*8,8))[0]
                ms=t if index==2 else t*62.5
                coeff=min(1,math.exp(math.log(.8)/(ms*.001*rate)))
                for channel in range(16):
                    shift=channel*0xc0
                    assert get((0x1a80 if index==2 else 0x1a70)+shift,'d')==ms
                    assert get((0x1a28 if index==2 else 0x1a30)+shift,'d')==coeff
                    if index==3:assert get(0x1a50+shift,'B')==int(enum==5)
                rows.append({'rate':rate,'index':index,'x':f32(x),'enum':enum,'milliseconds':ms,'coefficient':coeff,'automatic':index==3 and enum==5})
    if setter:r.STUBS[0x141ac69e0]=setter
    return rows


def solid_splines():
    # Original constructor tables, shared grid initializer 1404d0830.
    tables={
        'BusComp:0':0x1479a7d60,'BusComp:1':0x1479a7dc0,
        'SolidEQ:bank0:0':0x1479a7f10,'SolidEQ:bank16:0':0x1479a7f70,
        'SolidEQ:bank0:1':0x1479a7fe0,'SolidEQ:bank16:1':0x1479a8040,
        'SolidEQ:bank0:3':0x1479a8760,'SolidEQ:bank16:3':0x1479a87c0,
        'SolidEQ:bank0:4':0x1479a89a0,'SolidEQ:bank16:4':0x1479a8a00,
        'SolidEQ:bank0:5':0x1479a8820,'SolidEQ:bank16:5':0x1479a8880,
        'SolidEQ:bank0:6':0x1479a8d60,'SolidEQ:bank16:6':0x1479a8dc0,
        'SolidEQ:bank0:7':0x1479a8ee0,'SolidEQ:bank16:7':0x1479a8f40,
        'SolidEQ:bank0:8':0x1479a8e20,'SolidEQ:bank16:8':0x1479a8e80,
        'SolidEQ:bank0:9':0x1479a92a0,'SolidEQ:bank16:9':0x1479a9300,
        'SolidEQ:bank0:10':0x1479a9360,'SolidEQ:bank16:10':0x1479a93c0,
        'SolidEQ:12':0x1479a9ba0,'SolidEQ:13':0x1479aa3e0}
    read=lambda a:struct.unpack('<d',r.PE.get_data(a-r.BASE,8))[0]
    grid=[float(f32(i*f32(0.1))) for i in range(11)]
    boundaries=[read(0x1479ab470),read(0x1479ab648)]
    threshold=read(0x1479ab638)
    rows=[]
    for name,address in tables.items():
        clear();put(0x20,'i',11);put(0x238,'B',1)
        CPU.mem_write(INPUT,struct.pack('<11d',*grid))
        call(0x141a9a510,[INPUT,address],[(3,boundaries[0],'d')],(boundaries[1],'d'),OBJ+0x20)
        y=[get(0x130+i*8,'d') for i in range(11)]
        m=[get(0x188+i*8,'d') for i in range(11)]
        assert all(math.isfinite(v) for v in y+m)
        # Spline boundary equations and interior continuity define the curve.
        if boundaries[0]>threshold: assert m[0]==0
        else: assert abs((2*m[0]+m[1])*(grid[1]-grid[0])-6*(y[1]-y[0])/(grid[1]-grid[0]))<1e-7
        if boundaries[1]>threshold: assert m[-1]==0
        else: assert abs((m[-2]+2*m[-1])*(grid[-1]-grid[-2])+6*(y[-1]-y[-2])/(grid[-1]-grid[-2]))<1e-7
        for i in range(1,10):
            a,b=grid[i]-grid[i-1],grid[i+1]-grid[i]
            residual=a*m[i-1]+2*(a+b)*m[i]+b*m[i+1]-6*((y[i+1]-y[i])/b-(y[i]-y[i-1])/a)
            assert abs(residual)<1e-7*max(1,abs(y[i]))
        vectors=[]
        for x in [-.1,0,.05,.25,.5,.75,.95,1,1.1,*grid]:
            put(0,'d',x);call(0x141a98340)
            actual=xmm(CPU,UC_X86_REG_XMM0,'d')
            xc=min(grid[-1],max(grid[0],x));i=min(9,next((i for i in range(10) if xc<grid[i+1]),9))
            h=grid[i+1]-grid[i];a=(grid[i+1]-xc)/h;b=(xc-grid[i])/h
            expected=a*y[i]+b*y[i+1]+((a**3-a)*m[i]+(b**3-b)*m[i+1])*h*h/6
            assert abs(actual-expected)<1e-9*max(1,abs(actual))
            vectors.append({'x':x,'internal':actual})
        rows.append({'parameter':name,'table_va':hex(address),'grid':grid,'knots':y,'second_derivatives':m,'boundary_controls':boundaries,'vectors':vectors})
    return rows


def external_modulator_queue():
    rows=[]
    for events in [[],[(0,-1.0)],[(0,-1.0),(2,.5),(5,1.0),(8,-.25)],[(12,.75)],[(0,.5),(0,-.5),(2,0.0)]]:
        for blocks in [[12],[0,1,2,3,4,2],[2,0,2,4,4]]:
            clear();put(0x1b0,'Q',len(events));put(0x1b8,'f',.25)
            for i,(tick,value) in enumerate(events):put(0xb0+i*8,'f',value);put(0xb4+i*8,'i',tick)
            pending=list(events);state=f32(.25);output=[]
            for n in blocks:
                CPU.mem_write(INPUT,b'\x55'*(4*(n+1)));call(0x140ceaf60,[INPUT,n])
                expected=[]
                for tick in range(n):
                    while pending and pending[0][0]==tick:state=f32(pending.pop(0)[1])
                    expected.append(state)
                pending=[(tick-n,value) for tick,value in pending]
                actual=struct.unpack('<'+'f'*n,CPU.mem_read(INPUT,n*4));assert actual==tuple(expected),(events,blocks,n,actual,expected)
                assert get(0x1b8,'f')==state and get(0x1b0,'Q')==len(pending)
                for i,(tick,value) in enumerate(pending):assert get(0xb4+i*8,'i')==tick and get(0xb0+i*8,'f')==f32(value)
                assert bytes(CPU.mem_read(INPUT+n*4,4))==b'\x55'*4
                output.extend(actual)
            rows.append({'initial':.25,'events':events,'blocks':blocks,'samples':output,'final':state,'pending':pending})
    return rows


def ahdsr_lifecycle():
    rows=[]
    for curve in [-1.0,0.0,1.0]:
        for lengths in [(7,3,11,13),(0,3,11,13),(0,0,11,13),(0,0,0,13),(7,0,0,0)]:
            for release_tick in [0,5,25]:
                for blocks in [[70],[0,1,3,4,5,31,26]]:
                    clear(); a,h,d,release=lengths;sustain=f32(.125)
                    b=f32(math.exp(f32(1-abs(f32(curve)))*math.log(500000)-math.log(20000)))
                    start=f32(b+1) if curve>0 else b
                    ratio=f32(b/start) if curve>0 else f32(f32(b+1)/b)
                    starts=[start,f32(1.075),f32(1.075),f32(sustain+f32(.075)),f32(1.075),f32(.075)]
                    durations=[a,h,d,0x7fffffff,release,0x7fffffff]
                    multipliers=[f32(ratio**(1/a)) if a else 0,1,f32((3/43)**(1/d)) if d else 0,1,f32((3/43)**(1/release)) if release else 0,1]
                    for i in range(6):
                        put(0xc4+i*4,'i',durations[i]);put(0xf4+i*4,'f',starts[i]);put(0xdc+i*4,'f',multipliers[i])
                    put(0x120,'f',curve);call(0x140ae5820)
                    stage=next(i for i,n in enumerate(durations) if n)
                    state=starts[stage];remaining=durations[stage];scale=f32(-1 if stage==0 and curve>0 else 1)
                    bias=f32(start-f32(.075)) if stage==0 and curve>0 else f32(f32(.075)-start) if stage==0 else sustain if stage==2 else 0
                    if stage==2:scale=f32(1-sustain)
                    assert get(0xc0,'i')==stage and get(0xb8,'f')==state and get(0x110,'f')==scale and get(0x114,'f')==bias
                    put(0x10c,'i',release_tick);left=release_tick;output=[];position=0
                    for frames in blocks:
                        expected=[]
                        for _ in range(frames):
                            if left==0 and stage<4:
                                old=f32(f32(f32(state-f32(.075))*scale)+bias);stage=4;state=starts[stage];remaining=durations[stage];scale=old;bias=0;left=0x7fffffff
                            while remaining==0 and stage<5:
                                stage+=1
                                old=f32(f32(f32(state-f32(.075))*scale)+bias)
                                state=starts[stage];remaining=durations[stage];scale=1;bias=0
                                if stage==2:scale=f32(1-sustain);bias=sustain
                                if stage==4:scale=old
                                if stage==5:scale=0
                            expected.append(f32(f32(f32(state-f32(.075))*scale)+bias));state=f32(state*multipliers[stage]);remaining-=1;left-=1
                            if remaining==0 and stage<5:
                                stage+=1
                                while durations[stage]==0 and stage<5:stage+=1
                                old=f32(f32(f32(state-f32(.075))*scale)+bias);state=starts[stage];remaining=durations[stage];scale=1;bias=0
                                if stage==2:scale=f32(1-sustain);bias=sustain
                                if stage==4:scale=old
                                if stage==5:scale=0
                        CPU.mem_write(INPUT,b'\x55'*(4*(frames+1)));call(0x140ae40f0,[INPUT,frames])
                        actual=struct.unpack('<'+'f'*frames,CPU.mem_read(INPUT,frames*4))
                        assert actual==tuple(expected),(curve,lengths,release_tick,blocks,frames,actual,expected)
                        assert bytes(CPU.mem_read(INPUT+frames*4,4))==b'\x55'*4
                        output.extend(actual);position+=frames
                    assert get(0xc0,'i')==stage and get(0xb8,'f')==state
                    rows.append({'curve':curve,'lengths':lengths,'release_tick':release_tick,'blocks':blocks,'samples':output,'final_stage':stage,'final_state':state})
    return rows


if __name__=='__main__':
    result={'image_sha256':r.META['sha256'],'image_kind':'standalone EXE; not payload',
            'mxcsr':'0x1f80','helper_substitutions':EXTRA,
            'ahdsr_controls':ahdsr_controls(),'ahdsr_kernel':ahdsr_kernel(),
            'galois_parameters':galois_parameters(),'eq_saved_conversion':eq_saved_conversion(),
            'solid_splines':solid_splines(),'stereo':stereo(),'wrapper_cadence':wrapper_cadence(),'eq_coefficients':eq_coefficients(),'stereo_smoothing':stereo_smoothing(),'bus_timing':bus_timing(),'ahdsr_lifecycle':ahdsr_lifecycle(),'external_modulator_queue':external_modulator_queue()}
    Path(sys.argv[2]).write_text(json.dumps(result,indent=2)+'\n')
    print({k:len(v) for k,v in result.items() if isinstance(v,list)})
