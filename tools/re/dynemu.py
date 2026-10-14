"""dynemu.py NAME [CONTROLS]: a native Kontakt 8 dynamics core under kemu; prints the outputs
at the frames the sampler-core dynamics native tests check (stimulus = tests.rs `stimulus()`)."""
import sys, math
sys.path.insert(0, __import__('os').path.dirname(__file__))
from kemu import Emu, f32

# name: (ctor list, prepare, setter, process, controls)
CORES = {
    'transient': ([0x141a83550, 0x141aa0b80], 0x141aa27a0, 0x141ac87b0, 0x141aaac90, {0: 0.9, 1: 0.2, 2: 0.6, 3: 0.0}),
    'hilbert': ([0x141adbea0, 0x141ae15b0], 0x141ae23a0, 0x141aea680, 0x141ae3400, {0: 0.5, 1: 0.05, 2: 1.0}),
    'bus': ([0x141a7fc80], 0x141aa1ea0, 0x141ac69e0, 0x141aaa1e0, {0: 0.2, 1: 0.4, 2: 0.5, 3: 0.5, 4: 0.6, 5: 1.0, 7: 1.0}),
    'feedback': ([0x141a7f3b0, 0x141a9e180], 0x141aa1280, 0x141ac56d0, 0x141aa80d0, {0: 0.6, 1: 0.5, 2: 0.3, 3: 0.4, 4: 0.5, 5: 1.0, 6: 1.0, 8: 1.0}),
}
# Fields the wrapper sets (transient +0x70: output scale, 1 = unity).
POKE = {'transient': {0x70: 1.0}}
VTABLE = {'feedback': 0x1479aad18, 'hilbert': 0x1479af050}
name = sys.argv[1]
ctors, prepare, setter, process, controls = CORES[name]
if len(sys.argv) > 2:
    controls = eval(sys.argv[2])
e = Emu('/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe')
blk = e.alloc(0x2000)
e.write(blk + 0xe04, 'i', 0x7fffffff)
arr = e.alloc(8 * 1024)
for i in range(1024):
    e.write(arr + 8 * i, 'Q', blk)
e.write(0x7D0000000000 + 0x58, 'Q', arr)
# Runtime-built exponent table 2^(i/60 - 20) (ar_kernel): [value, next - value] pairs.
t = [f32(2.0 ** (i / 60 - 20)) for i in range(2401)]
for i in range(2400):
    e.write(0x14a6b2600 + 8 * i, 'f', t[i], f32(t[i + 1] - t[i]))
e.write(0x14a6b7100, 'f', t[2400])
e.write(0x14a6521a8, 'd', 2.0 ** -24)  # G_BusComp static
obj = e.alloc(0x20000)
if name in VTABLE:
    e.write(obj, 'Q', VTABLE[name])
for c in ctors:
    e.call(c, obj)
e.write(obj + 0xc, 'I', 2)
e.write(obj + 0x10, 'I', 2)
e.call(prepare, obj, 0, 64, f={1: 48000.0})
if name == 'feedback':
    e.write(obj + 8, 'B', 1)  # snap parameter ramps (as a freshly reset core)
for off, v in POKE.get(name, {}).items():
    e.write(obj + off, 'f', v)
for k, v in controls.items():
    e.call(setter, obj, k, 0, f={2: v})
N, B = 4096, 64
inl, inr, outl, outr = [e.alloc(4 * B) for _ in range(4)]
ins, outs = e.alloc(16), e.alloc(16)
e.write(ins, 'Q', inl, inr)
e.write(outs, 'Q', outl, outr)
res = []
for b in range(N // B):
    for i in range(B):
        n = b * B + i
        env = 1.2 if (n // 1024) % 2 == 0 else 0.05
        e.write(inl + 4 * i, 'f', f32(env * math.sin(n * 0.0576)))
        e.write(inr + 4 * i, 'f', f32(env * 0.5 * math.cos(n * 0.1305)))
    e.call(process, obj, ins, outs, B)
    for i in range(B):
        res.append((e.read(outl + 4 * i, 'f'), e.read(outr + 4 * i, 'f')))
for n in [0, 1, 10, 100, 1000, 1500, 2047, 2500, 3000, 4095]:
    print('(%d, %r, %r),' % (n, res[n][0], res[n][1]))
