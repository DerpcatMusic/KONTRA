#!/usr/bin/env python3
"""Reproduce the rejected synthetic fixture experiment in tmpfs.

Uses only the existing, authored calibration.nki. The small encoders below
preserve its opaque state, changing groups, zones and their bounded records.
Kontakt rejects edited output; this is research input, not a working NKI writer.
"""
import copy
import hashlib
import itertools
import json
import struct
import sys
import zlib
from pathlib import Path


def u32(n):
    return struct.pack('<I', n)


def fastlz1(b):
    out = bytearray()
    i, ctrl = 1, b[0] & 31
    assert b[0] >> 5 == 0, 'fixture must use FastLZ level 1'
    while True:
        if ctrl < 32:
            n = ctrl + 1
            out += b[i:i+n]
            i += n
        else:
            n, ref = (ctrl >> 5) - 1, len(out) - ((ctrl & 31) << 8) - 1
            if n == 6:
                n += b[i]
                i += 1
            ref -= b[i]
            i += 1
            for j in range(n + 3):
                out.append(out[ref+j])
        if i == len(b):
            return bytes(out)
        assert i < len(b)
        ctrl, i = b[i], i + 1


def data_read(b):
    n, _, kind, version = struct.unpack_from('<Q4sII', b)
    assert n <= len(b) and version == 1
    node = [b[:20], None, b[20:n]]
    if kind != 1:
        node[1], used = data_read(b[20:n])
        node[2] = b[20+used:n]
    return node, n


def data_write(node):
    h, inner, tail = node
    body = (data_write(inner) if inner else b'') + tail
    return struct.pack('<Q', len(body)+20) + h[8:] + body


def item_read(b):
    n = struct.unpack_from('<Q', b)[0]
    assert b[12:16] == b'hsin' and n <= len(b)
    data, used = data_read(b[40:n])
    at = 40 + used
    version, count = struct.unpack_from('<II', b, at)
    assert version == 1
    at += 8
    kids = []
    for _ in range(count):
        header = b[at:at+12]
        child, used = item_read(b[at+12:n])
        kids.append([header, child])
        at += 12 + used
    return [b[:40], data, kids, b[at:n]], n


def item_write(node):
    h, data, kids, tail = node
    body = data_write(data) + u32(1) + u32(len(kids))
    body += b''.join(h + item_write(c) for h, c in kids) + tail
    return struct.pack('<Q', len(body)+40) + h[8:] + body


def chunks(b):
    out, at = [], 0
    while at < len(b):
        kind, n = struct.unpack_from('<HI', b, at)
        assert at + 6 + n <= len(b)
        out.append([kind, b[at+6:at+6+n]])
        at += 6+n
    assert at == len(b)
    return out


def chunk_write(nodes):
    return b''.join(struct.pack('<HI', kind, len(b)) + b for kind, b in nodes)


def so_read(b):
    assert b[0] == 1
    out, at = [b[:3]], 3
    for _ in range(3):
        n = struct.unpack_from('<I', b, at)[0]
        at += 4
        out.append(b[at:at+n])
        at += n
    assert at <= len(b)
    return out, at


def so_write(node):
    return node[0] + b''.join(u32(len(b))+b for b in node[1:])


def replace(nodes, kind, b):
    hits = [node for node in nodes if node[0] == kind]
    assert len(hits) == 1
    hits[0][1] = b


def loop(slot=0, mode=3, start=8192, length=4096, count=2, alternate=False, tune=1., fade=0):
    return dict(slot=slot, mode=mode, start=start, length=length, count=count,
                alternate=alternate, tune=tune, fade=fade)


def cases():
    out = [dict(name='unlooped', loops=[])]
    for mode in (1, 2, 3):
        for count in (0, 1, 2, 3):
            for alt in (False, True):
                out.append(dict(name=f'mode{mode}_count{count}_alt{int(alt)}',
                                loops=[loop(mode=mode, count=count, alternate=alt)]))
    for tune in (0., .5, 1., 2., 12.):
        out.append(dict(name=f'tune{tune:g}', loops=[loop(mode=1, tune=tune)]))
    for alt in (False, True):
        out.append(dict(name=f'fade512_alt{int(alt)}', loops=[loop(mode=1, alternate=alt, fade=512)]))
    for slot in range(1, 8):
        out.append(dict(name=f'slot{slot}', loops=[loop(slot=slot)]))
    for slots in ((0, 1, 7), (1, 7), (1, 3, 7)):
        out.append(dict(name='serial'+''.join(map(str, slots)), loops=[
            loop(slot=s, start=8192+8192*i, length=2048, count=2)
            for i, s in enumerate(slots)]))
    # Later physical slot starts earlier in the source: separates slot order
    # from sorting loop starts. A counted first loop can advance to the next.
    out.append(dict(name='serial71_positions', loops=[loop(slot=1,start=24576,length=2048),
                                                     loop(slot=7,start=8192,length=2048)]))
    for joins in itertools.product(range(3), repeat=2):
        out.append(dict(name='criteria'+''.join(map(str, joins)), joins=list(joins), loops=[]))
    for i, c in enumerate(out):
        c['key'] = 24+i
    assert out[-1]['key'] <= 127
    return out


def prepare(directory):
    assert str(directory).startswith('/dev/shm/'), 'authored fixtures must stay in tmpfs'
    template = Path(__file__).resolve().parents[3] / 'tools/kontakt-reference/scenarios/calibration.nki'
    raw = template.read_bytes()
    root, n = item_read(raw)
    assert n == len(raw) and item_write(root) == raw
    subtree = root[2][0][1][2][2][1][1][1]  # Preset -> packed child -> SubtreeItem
    assert struct.unpack_from('<I', subtree[0], 12)[0] == 0x73
    packed = subtree[2]
    assert packed[:5] == b'\x01\0\0\0\x01'
    expanded, length = struct.unpack_from('<II', packed, 5)
    decoded = fastlz1(packed[13:13+length])
    assert len(decoded) == expanded
    inner, n = item_read(decoded)
    assert n == len(decoded) and item_write(inner) == decoded
    props = inner[2][0][1][1][2]
    size = struct.unpack_from('<Q', props, 12)[0]
    original = chunks(props[20:20+size])
    program, _ = so_read(original[0][1])
    children = chunks(program[3])
    group, _ = so_read(dict(children)[0x33][4:])
    zone, _ = so_read(dict(children)[0x34][8:])
    directory.mkdir(parents=True, exist_ok=True)
    spec = cases()
    groups, zones = [], []
    for index, case in enumerate(spec):
        g, z = copy.deepcopy(group), copy.deepcopy(zone)
        gc, zc = chunks(g[3]), chunks(z[3])
        records = []
        if 'joins' in case:
            for row in range(3):
                join = case['joins'][row] if row < 2 else 0
                records.append(b'\0\x70\0' + struct.pack('<iihhhhhiiiB',
                               2, join, 0, 127, 21+row, 127, 127, 0, 0, 0, 0))
        replace(gc, 0x38, bytes([(1 << len(records))-1]) + b''.join(records))
        loops = sorted(case['loops'], key=lambda l:l['slot'])
        payload = bytes([sum(1 << l['slot'] for l in loops)])
        for l in loops:
            payload += b'\0\x60\0' + struct.pack('<iiiiBfi', l['mode'], l['start'],
                       l['length'], l['count'], l['alternate'], l['tune'], l['fade'])
        replace(zc, 0x39, payload)
        zp = bytearray(z[2])
        struct.pack_into('<hh', zp, 16, case['key'], case['key'])
        struct.pack_into('<h', zp, 28, case['key'])
        g[3], z[3], z[2] = chunk_write(gc), chunk_write(zc), bytes(zp)
        groups.append(so_write(g))
        zones.append(u32(index) + so_write(z))
    replace(children, 0x33, u32(len(groups))+b''.join(groups))
    replace(children, 0x34, u32(len(zones))+b''.join(zones))
    program[3] = chunk_write(children)
    original[0][1] = so_write(program)
    patched = chunk_write(original)
    inner[2][0][1][1][2] = props[:12] + struct.pack('<Q', len(patched)) + patched + props[20+size:]
    # Use legal FastLZ level-1 literal runs; retain the native packed wrapper.
    # ponytail: no match compressor needed for this tiny authored fixture.
    expanded = item_write(inner)
    literal = b''.join(bytes([len(expanded[i:i+32])-1])+expanded[i:i+32]
                       for i in range(0, len(expanded), 32))
    assert fastlz1(literal) == expanded
    subtree[2] = u32(1) + b'\1' + u32(len(expanded)) + u32(len(literal)) + literal
    # Authored template's BPatchHeaderV42 payload CRC covers the inner chunks.
    header = bytearray(root[2][0][1][2][3][1][1][2])
    assert struct.unpack_from('<I', header, 182)[0] == zlib.crc32(props[20:20+size])
    struct.pack_into('<I', header, 182, zlib.crc32(patched))
    root[2][0][1][2][3][1][1][2] = bytes(header)
    out = directory / 'vectors.nki'
    out.write_bytes(item_write(root))
    check, n = item_read(out.read_bytes())
    assert n == out.stat().st_size and item_write(check) == out.read_bytes()
    (directory/'cases.json').write_text(json.dumps(spec, indent=2)+'\n')
    print(json.dumps(dict(template_sha256=hashlib.sha256(raw).hexdigest(),
                         fixture_sha256=hashlib.sha256(out.read_bytes()).hexdigest(), cases=len(spec))))


if __name__ == '__main__':
    prepare(Path(sys.argv[1]))
