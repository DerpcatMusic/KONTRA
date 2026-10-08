#!/usr/bin/env python3
"""Evict only original source files in one library; verify with mincore, no reads."""
import ctypes
import json
import os
from pathlib import Path
import sys

libc = ctypes.CDLL(None, use_errno=True)
libc.mmap.restype = ctypes.c_void_p
libc.mmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_long]
libc.mincore.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
libc.munmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t]

def resident(fd, size):
    if not size:
        return 0
    ptr = libc.mmap(None, size, 1, 1, fd, 0)
    if ptr == ctypes.c_void_p(-1).value:
        raise OSError(ctypes.get_errno())
    pages = (size + 4095) // 4096
    vec = (ctypes.c_ubyte * pages)()
    try:
        if libc.mincore(ptr, size, vec):
            raise OSError(ctypes.get_errno())
        return sum(x & 1 for x in vec)
    finally:
        libc.munmap(ptr, size)

files = [p for p in Path(sys.argv[1]).rglob('*') if p.is_file() and p.suffix.lower() in {'.nkx', '.ncw', '.wav', '.nki', '.nkr', '.ufs'}]
before = after = total = 0
for path in files:
    fd = os.open(path, os.O_RDONLY)
    try:
        size = os.fstat(fd).st_size
        before += resident(fd, size)
        os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
        after += resident(fd, size)
        total += (size + 4095) // 4096
    finally:
        os.close(fd)
print(json.dumps(dict(files=len(files), pages_total=total, pages_before=before, pages_after=after)))
