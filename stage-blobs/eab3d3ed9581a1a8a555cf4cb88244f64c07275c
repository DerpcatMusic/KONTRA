"""Count sample containers the filesystem cannot fully map. See audits/NTFS.md.

A file is affected when FIEMAP maps fewer bytes than its size (the driver dropped
runlist extents) or a 64 KiB probe at 25/50/75/100% reads all zeros. Reads a few
blocks per file; prints paths only, never sample data.
usage: python3 tools/ntfs_zero_scan.py [root=/path/to/Libraries] [-v]
"""
import collections
import fcntl
import os
import struct
import sys

FS_IOC_FIEMAP = 0xC020660B
PROBE = 65536
args = [a for a in sys.argv[1:] if a != '-v']
root = args[0] if args else '/path/to/Libraries'


def mapped_bytes(fd, size):
    mapped, start = 0, 0
    while start < size:
        buf = bytearray(struct.pack('QQIIII', start, size - start, 0, 0, 256, 0) + bytes(56 * 256))
        fcntl.ioctl(fd, FS_IOC_FIEMAP, buf)
        count = struct.unpack_from('I', buf, 20)[0]
        if not count:
            break
        for i in range(count):
            logical, _, length, _, _, flags = struct.unpack_from('QQQQQI', buf, 32 + 56 * i)
            mapped, start = mapped + length, logical + length
        if flags & 1:  # FIEMAP_EXTENT_LAST
            break
    return min(mapped, size)


def zero_probes(fd, size):
    if size <= 4 * PROBE:
        return False
    offsets = (min(int(size * f), size - PROBE) & ~4095 for f in (0.25, 0.5, 0.75, 1.0))
    return any(os.pread(fd, PROBE, o).count(0) == PROBE for o in offsets)


files, total, short, zeros, per_lib = 0, 0, 0, 0, collections.Counter()
for folder, _, names in os.walk(root):
    for name in names:
        if not name.lower().endswith(('.nkx', '.ncw', '.nkc', '.nkr')):
            continue
        path = os.path.join(folder, name)
        size = os.path.getsize(path)
        fd = os.open(path, os.O_RDONLY)
        try:
            is_short = size > 4096 and mapped_bytes(fd, size) + PROBE < size
            is_zero = zero_probes(fd, size)
        finally:
            os.close(fd)
        files, total = files + 1, total + size
        short += is_short
        zeros += is_zero
        if is_short:
            per_lib['/'.join(os.path.relpath(path, root).split(os.sep)[:2])] += size
            if '-v' in sys.argv:
                print('short', path)
print(f'{files} files, {total / 1e9:.1f} GB; unmapped tail: {short} files ({sum(per_lib.values()) / 1e9:.1f} GB); zero probe: {zeros}')
for lib, size in sorted(per_lib.items()):
    print(f'  {size / 1e9:7.1f} GB  {lib}')
sys.exit(1 if short else 0)
