"""Kontakt 8.13.1 AET filter (BEffectDYX<N>) reference model.

engine(): the 44.1 kHz block engine 0x140850400 + 128-frame buffering of 0x14090c1d0, per channel:
  acc (3200 f32) <- shift by 128 (zero tail); output = acc[0:128] of the previous block (latency 128).
  dry:   acc[n] += ((1-amt)*W(cur,pos) + amt) * x[n]                                  (0x140a9d900)
  layer: for <= 2 morph layers L != cur with b0 <= pos < b4 (list order; 1st -> slot 0, last other -> slot 1):
         t[0:128] = ((1-amt)*ratio_L*W(L,pos)) * x, t[128:3200] = 0                    (0x140a96e60)
         T = rfft3200(t);  T = B_L * (A_cur * T)  (complex f32 per bin, 1601 bins)      (0x140a98430)
         acc[0:3200] += (1/3200) * irfft3200_unnormalised(T)                            (0x140a9d900, 0x39a3d70a)
  ratio_L = L.level / cur.level   (cur.level >= 1e-5), else (1 if L.level < 1e-5 else 0)  [0x1447148a0]
  No layer in range (or no list) -> acc[0:128] += 1.0 * x (no ramp, pos untouched).
W(node,pos), 0x140883120: breakpoints b0..b4 = node floats [3..7]; b2 < 0 -> 1. In [b_k, b_k+1):
  u = (pos-b_k)/(b_k+1 - b_k), s = sinf(u*1.57079637); k=0: 0.5 s^2, 1: 0.5(s^2+1), 2: 1-0.5 s^2, 3: 0.5-0.5 s^2.
  Outside all segments -> 0.
Ramp (per 128 block): if |pos+1e-12|/|tgt+1e-12| or |amt+1e-12|/|atgt+1e-12| outside [0.999, 1.001]:
  s = clamp((tgt-pos)/128, +-0.00226757373), ds likewise for amount; end e = 128 s + pos is pushed out of the
  [b0,b4) range of cur / slot0 / slot1 (if pos is inside it and e is not): e > b4 -> b4 + 0.00226757, e < b0 ->
  b0 - 0.00226757; then s = clamp((e-pos)/128). Per sample: gains use pos/amt before the step;
  pos += s (unclamped), amt = clamp01(amt + ds). After the block pos is clamped to [0,1].
FFT: FFTW single precision (plans from wisdom; modelled here with float64 numpy rounded to f32)."""
import numpy as np
f = np.float32
NFFT, NB, HOP = 3200, 1601, 128
HALF_PI = f(1.57079637)
STEP = f(0.00226757373)            # 0x144f38618 (3b149b93)
INV128 = f(0.0078125)              # 0x144e79a00
OLA = f(0.000312499993)            # 0x144f385f0 (39a3d70a)

class Layer:
    """level = node[0]; bp = (b0..b4) = node[3..7]; A[set][ch] / B[set][ch]: complex64[1601].
    A = spectrum used when this layer is the current one (+0x30 / +0x40), B when it is the target (+0x38 / +0x48)."""
    def __init__(self, level, bp, A, B):
        self.level, self.bp = f(level), [f(b) for b in bp]
        self.A, self.B = A, B

def W(node, pos):
    b = node.bp
    if not b[2] >= 0: return f(1)
    for k in range(4):
        if b[k] <= pos < b[k + 1]:
            s = f(np.sin(np.float64(f(f(pos - b[k]) / f(b[k + 1] - b[k])) * HALF_PI)))
            s2 = f(s * s)
            return [f(s2 * f(0.5)), f(f(s2 + f(1)) * f(0.5)), f(f(1) - f(s2 * f(0.5))), f(f(0.5) - f(s2 * f(0.5)))][k]
    return f(0)

def _clamp(v): return f(min(max(v, -STEP), STEP))

class Engine:
    def __init__(self, layers, cur, morph, amount, nch=1, set1=False):
        self.L, self.cur = layers, layers[cur] if layers else None
        self.pos = self.tgt = f(morph); self.amt = self.atgt = f(amount)
        self.nch, self.set = nch, int(set1)
        self.acc = np.zeros((nch, NFFT), f); self.inb = np.zeros((nch, HOP), f); self.fill = 0

    def block(self):
        acc, x = self.acc, self.inb
        acc[:, :NFFT - HOP] = acc[:, HOP:].copy(); acc[:, NFFT - HOP:] = 0
        cur = self.cur
        slot = [None, None]
        for nd in self.L:
            if nd is not cur and nd.bp[0] <= self.pos < nd.bp[4]:
                if slot[0] is None: slot[0] = nd
                else: slot[1] = nd
        if cur is None or slot[0] is None:
            for c in range(self.nch): acc[c, :HOP] = (f(1) * x[c] + acc[c, :HOP]).astype(f)
            return
        pos0, amt0 = self.pos, self.amt
        s = ds = f(0)
        r1 = f(abs(f(pos0 + f(1e-12)))) / f(abs(f(self.tgt + f(1e-12))))
        r2 = f(abs(f(amt0 + f(1e-12)))) / f(abs(f(self.atgt + f(1e-12))))
        if r1 > f(1.001) or r1 < f(0.999) or r2 > f(1.001) or r2 < f(0.999):
            s = _clamp(f(f(self.tgt - pos0) * INV128)); ds = _clamp(f(f(self.atgt - amt0) * INV128))
            if s != 0 or ds != 0:
                e = f(f(s * f(128)) + pos0)
                for nd in (cur, slot[0], slot[1]):
                    if nd is None or (nd is not cur and nd is cur): continue
                    b0, b4 = nd.bp[0], nd.bp[4]
                    if b0 <= pos0 < b4 and (e < b0 or e >= b4):
                        if e > b4: e = f(b4 + STEP)
                        if e < b0: e = f(b0 - STEP)
                s = _clamp(f(f(e - pos0) * INV128))
        ramp = not (s == 0 and ds == 0)

        def gains(node, kind, ratio=f(1)):
            """per-sample gain list (and the final pos/amt) for the dry (kind 0) or a layer (kind 1) path."""
            pos, amt = pos0, amt0
            if not ramp:
                w = W(node, pos)
                g = f(f(f(f(1) - amt) * w) + amt) if kind == 0 else f(f(f(f(1) - amt) * ratio) * w)
                return np.full(HOP, g, f), pos, amt
            out = np.empty(HOP, f)
            for n in range(HOP):
                w = W(node, pos); a = amt
                pos = f(pos + s); amt = f(min(max(f(a + ds), f(0)), f(1)))
                out[n] = f(f(f(f(1) - a) * w) + a) if kind == 0 else f(f(f(f(1) - a) * ratio) * w)
            return out, pos, amt

        g, pend, aend = gains(cur, 0)
        for c in range(self.nch): acc[c, :HOP] = (g * x[c] + acc[c, :HOP]).astype(f)
        for nd in slot:
            if nd is None or nd is cur: continue
            if cur.level >= f(1e-5): ratio = f(nd.level / cur.level)
            else: ratio = f(0) if nd.level >= f(1e-5) else f(1)
            g, pend, aend = gains(nd, 1, ratio)
            for c in range(self.nch):
                t = np.zeros(NFFT, f); t[:HOP] = (g * x[c]).astype(f)
                X = np.fft.rfft(t.astype(np.float64)).astype(np.complex64)
                A = cur.A[self.set][c % len(cur.A[self.set])]; B = nd.B[self.set][c % len(nd.B[self.set])]
                Xr, Xi, Ar, Ai, Br, Bi = X.real, X.imag, A.real, A.imag, B.real, B.imag
                tr = (Ar * Xr - Ai * Xi).astype(f); ti = (Ar * Xi + Ai * Xr).astype(f)
                Yr = (Br * tr - Bi * ti).astype(f); Yi = (Bi * tr + Br * ti).astype(f)
                y = (np.fft.irfft(Yr.astype(np.float64) + 1j * Yi.astype(np.float64), NFFT) * NFFT).astype(f)
                acc[c] = (OLA * y + acc[c]).astype(f)
        self.pos, self.amt = pend, aend
        self.pos = f(1) if self.pos > f(1) else (f(0) if self.pos < f(0) else self.pos)

    def run(self, X):
        """X: (nch, n) input at 44.1 kHz -> (nch, n) output (0x14090c1d0 buffering, 128-sample latency)."""
        X = np.asarray(X, f).reshape(self.nch, -1); n = X.shape[1]; Y = np.zeros_like(X); i = 0
        while i < n:
            k = min(n - i, HOP - self.fill)
            self.inb[:, self.fill:self.fill + k] = X[:, i:i + k]
            Y[:, i:i + k] = self.acc[:, self.fill:self.fill + k]
            self.fill += k; i += k
            if self.fill == HOP: self.block(); self.fill = 0
        return Y

def engine(layers, cur, morph, amount, x, nch=1, set1=False):
    """Fresh-state run at 44.1 kHz. layers: [Layer]; cur: index of the zone's own layer; x: (nch, n)."""
    return Engine(layers, cur, morph, amount, nch, set1).run(x)
