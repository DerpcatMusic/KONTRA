#!/usr/bin/env python3
"""Verify PE provenance and execute the native public readers with stubbed I/O.

Requires the already-installed pefile and unicorn. No Wine session, instrument,
sample, build, or prefix write. This checks serialization, not audio semantics.
"""
import hashlib
import json
import struct
from pathlib import Path

import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_64, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_RAX, UC_X86_REG_RCX, UC_X86_REG_RDX,
                              UC_X86_REG_R8, UC_X86_REG_R9, UC_X86_REG_RSP,
                              UC_X86_REG_RIP, UC_X86_REG_RBP, UC_X86_REG_XMM0)


def main():
    metadata = json.loads(Path(__file__).with_name('provenance.json').read_text())
    raw = Path(metadata['binary']).read_bytes()
    assert hashlib.sha256(raw).hexdigest() == metadata['sha256']
    pe = pefile.PE(data=raw, fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    for entry in metadata['ranges']:
        offset = pe.get_offset_from_rva(int(entry['va'], 16)-base)
        assert hashlib.sha256(raw[offset:offset+entry['bytes']]).hexdigest() == entry['sha256']
    u = Uc(UC_ARCH_X86, UC_MODE_64)
    u.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage+4095) & ~4095)
    u.mem_write(base, pe.get_memory_mapped_image())
    u.mem_map(0x100000, 0x200000)
    obj, stop, stack = 0x200000, 0x100000, 0x2fff00
    source, cursor, tag = b'', 0, 0

    def take(n):
        nonlocal cursor
        assert cursor+n <= len(source), 'native reader passed synthetic input boundary'
        value = source[cursor:cursor+n]
        cursor += n
        return value

    def ret(value=0):
        rsp = u.reg_read(UC_X86_REG_RSP)
        target = struct.unpack('<Q', u.mem_read(rsp, 8))[0]
        u.reg_write(UC_X86_REG_RAX, value)
        u.reg_write(UC_X86_REG_RSP, rsp+8)
        u.reg_write(UC_X86_REG_RIP, target)

    widths = {0x142a780c0: 1, 0x142a78220: 2,
              0x142a78270: 4, 0x142a780f0: 4}

    def hook(uc, address, size, _):
        if address in widths:
            value = int.from_bytes(take(widths[address]), 'little')
            if address == 0x142a780f0:
                u.reg_write(UC_X86_REG_XMM0, value)
            ret(value)
        elif address == 0x142a78330:  # byte-string I/O, no native allocation
            take(int.from_bytes(take(4), 'little'))
            destination = u.reg_read(UC_X86_REG_RDX)
            u.mem_write(destination, bytes(24)+struct.pack('<Q', 15))
            ret(destination)
        elif address == 0x14096af20:  # tag lookup is separately proven by PE table constants
            ret(tag)
        elif address == 0x140cfbaf0:  # wide-string I/O, no native allocation
            take(2*int.from_bytes(take(4), 'little'))
            ret()
        elif address == 0x1443c9590:  # security-cookie check, unrelated to reader logic
            ret()
        elif address == stop:
            u.emu_stop()
        elif address in (0x1407fab50, 0x140a874c0):
            raise AssertionError('native rejected version or enum')

    u.hook_add(UC_HOOK_CODE, hook)

    def run(va, version, data, enum=0):
        nonlocal source, cursor, tag
        source, cursor, tag = data, 0, enum
        u.mem_write(obj, bytes(0x400))
        u.mem_write(obj+0x3c, struct.pack('<i', -1))
        u.mem_write(stack, struct.pack('<Q', stop))
        for reg, value in ((UC_X86_REG_RCX, obj), (UC_X86_REG_RDX, 0x210000),
                           (UC_X86_REG_R8, version), (UC_X86_REG_R9, 0),
                           (UC_X86_REG_RSP, stack)):
            u.reg_write(reg, value)
        u.emu_start(va, stop, count=100000)
        assert u.reg_read(UC_X86_REG_RIP) == stop
        assert cursor == len(data), (hex(va), cursor, len(data))
        return bytes(u.mem_read(obj, 0x400))

    def i32(data, offset):
        return struct.unpack_from('<i', data, offset)[0]

    rows = ['reader\tversion\tinput\tconsumed_bytes\tverified_output']
    for join in range(3):
        payload = struct.pack('<iihhhhhiiiB', 2, join, 0, 127, 21, 127, 127, 0, 0, 0, 0)
        result = run(0x140d04400, 0x70, payload)
        assert i32(result, 0x28) == join
        rows.append(f'criteria\t0x70\tjoin={join}\t{len(payload)}\tobject+0x28={join}')
    for mode, internal in enumerate((0, 3, 4, 1)):
        payload = struct.pack('<iiiiBfi', mode, 8192, 4096, 2, True, .5, 512)
        result = run(0x140cff510, 0x60, payload)
        assert [i32(result, off) for off in (0x20, 8, 12, 24, 40)] == [internal, 8192, 4096, 2, 512]
        assert result[28] == 1 and struct.unpack_from('<f', result, 36)[0] == .5
        rows.append(f'loop\t0x60\tmode={mode},start=8192,len=4096,count=2,alt=1,tune=.5,fade=512\t{len(payload)}\tinternal_mode={internal};fields_preserved')
    for mode, address in ((1, 1), (2, 10), (2, 65535)):
        name = b'pts_script_slider_0_40'
        payload = struct.pack('<IBHiiffI', mode, 0, address, 0, -1, 0., 1., len(name))+name
        result = run(0x140cfec70, 0x71, payload, 0x17db)
        assert [i32(result, off) for off in (0x28, 0x34, 0x40, 0x44)] == [mode, 0x17db, 0, -1]
        assert result[0x31] == 0
        assert struct.unpack_from('<H', result, 0x32)[0] == address
        rows.append(f'automation\t0x71\tmode={mode},takeover=0,address={address},obj=0,chain=-1,range=0..1,slider_0_40\t{len(payload)}\tfields_preserved;tag_lookup_stub=0x17db')
    name = 'test'.encode('utf-16le')
    payload = struct.pack('<iI', 7, len(name)//2)+name+struct.pack('<fffBH', .1, .2, .3, 1, 17)
    result = run(0x140cfedf0, 0x50, payload)
    assert i32(result, 0x28) == 7 and result[0x4c] == 1
    rows.append(f'prefix_array_A\t0x50\tUTF16_units=4\t{len(payload)}\t23+2*N_bytes')
    payload = struct.pack('<iiI', 1, 7, len(name)//2)+name+struct.pack('<Iiii', 3, 11, 22, 33)
    result = run(0x140cfeea0, 0x50, payload)
    assert [i32(result, off) for off in (8, 12, 0x38, 0x3c, 0x40)] == [1, 7, 11, 22, 33]
    rows.append(f'prefix_array_B\t0x50\tUTF16_units=4,K=3\t{len(payload)}\t16+2*N+4*K_bytes')
    output = '\n'.join(rows)+'\n'
    expected = Path(__file__).with_name('reader_vectors.tsv').read_text()
    assert output == expected, 'native results differ from checked-in vectors'

    bounds = ['path\tmode\taddress\tdelta\tobject_flag\tverified_output']
    for address in (0, 2047, 2048, 2049, 65535):
        u.reg_write(UC_X86_REG_RBP, 0x230000)
        u.mem_write(0x230138, struct.pack('<H', address))
        # Four original instructions; stop immediately after the conditional branch.
        u.emu_start(0x1408f3479, 0x1408f3490, count=4)
        allowed = u.reg_read(UC_X86_REG_RIP) == 0x1408f3490
        assert u.reg_read(UC_X86_REG_RIP) in (0x1408f3490, 0x1408f3da3)
        assert allowed == (address < 2049)
        bounds.append(f'creation_address_gate\tany\t{address}\t-\t-\t{"allowed" if allowed else "rejected"}')
    assert struct.unpack('<I', u.mem_read(0x144fd4c80, 4))[0] == 2049
    program, group, group_list, group_bao = 0x210000, 0x240000, 0x260000, 0x250000
    for mode, address, delta, flag, expected in (
            (2, 0, 0, 0, 0), (2, 2047, 0, 0, 2047),
            (2, 2048, 0, 0, 2048), (2, 2049, 0, 0, 2048),
            (2, 65535, 0, 0, 2048), (2, 0, -1, 0, 0),
            (2, 1, -1, 0, 0), (2, 2047, 1, 0, 2048),
            (2, 2048, 1, 0, 2048), (1, 65535, 0, 0, 65535),
            (2, 65535, 0, 1, 65535)):
        u.mem_write(program, bytes(0x1d000))
        u.mem_write(group, bytes(0x200))
        for target in (obj, group_bao):
            u.mem_write(target, bytes(0x60))
            u.mem_write(target+8, bytes([flag]))
            u.mem_write(target+0x28, struct.pack('<i', mode))
            u.mem_write(target+0x32, struct.pack('<H', address))
        for target, offset, fmt, value in (
                (program, 0x1cee8, 'Q', obj), (program, 0x1cf0c, 'i', 1),
                (program, 0x1cf30, 'i', delta), (program, 0x11370, 'Q', group_list),
                (program, 0x11378, 'Q', group_list+8), (group, 0xe8, 'Q', group_bao),
                (group, 0x10c, 'i', 1), (group_list, 0, 'Q', group)):
            u.mem_write(target+offset, struct.pack('<'+fmt, value))
        u.mem_write(stack, struct.pack('<Q', stop))
        u.reg_write(UC_X86_REG_RCX, program)
        u.reg_write(UC_X86_REG_RSP, stack)
        u.emu_start(0x1409ab730, stop, count=1000)
        assert u.reg_read(UC_X86_REG_RIP) == stop
        for target in (obj, group_bao):
            assert struct.unpack('<H', u.mem_read(target+0x32, 2))[0] == expected
        bounds.append(f'program_and_group_remap\t{mode}\t{address}\t{delta}\t{flag}\taddress={expected}')
    assert '\n'.join(bounds)+'\n' == Path(__file__).with_name('host_address_vectors.tsv').read_text()
    print(f'PASS: {len(metadata["ranges"])} original PE ranges; {len(rows)-1} native-reader vectors (stubbed I/O); {len(bounds)-1} native address-bound vectors.')


if __name__ == '__main__':
    main()
