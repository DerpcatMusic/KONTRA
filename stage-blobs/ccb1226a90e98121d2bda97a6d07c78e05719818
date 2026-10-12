import struct, numpy as np
def rd(p):
    b = open(p, "rb").read(); i = 12; fmt = None
    while i + 8 <= len(b):
        t, n = b[i:i+4], struct.unpack("<I", b[i+4:i+8])[0]
        if t == b"fmt ": fmt = struct.unpack("<HHIIHH", b[i+8:i+24])
        if t == b"data":
            raw = b[i+8:i+8+n]
            if fmt[0] == 3: return np.frombuffer(raw[:n//8*8], "<f4").reshape(-1, fmt[1]).astype(float), fmt[2]
            k = fmt[5]//8; a = np.frombuffer(raw[:n//(k*fmt[1])*k*fmt[1]], np.uint8).reshape(-1, k)
            v = (a[:, 0].astype(np.int32) | a[:, 1].astype(np.int32) << 8 | a[:, 2].astype(np.int32) << 16); v = np.where(v >= 1 << 23, v - (1 << 24), v)
            return (v / 2**23).reshape(-1, fmt[1]), fmt[2]
        i += 8 + n
