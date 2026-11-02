#!/usr/bin/env python3
"""Where a library file sits: the corpus root and the product folder it belongs to.

`build_tsv.py` and `ui_sample.py` both need the product folder a `.nki` belongs
to, and both used to read it out of a fixed position in the path —
`path.split("/")[6]` and `parts.index("VSTs")`. Those fit one layout exactly
(`/home/<user>/Armory/VSTs/Kontakt1/<product>/…`) and raise `IndexError`, or
quietly return the wrong component, for any other: a library root one level
shallower, a corpus under `/mnt`, a path whose `Instruments` level is missing.
The position is derived here instead, from the root, so only the root has to be
right.

The root is `LIBRARY_ROOT`, defaulting to the directory the 2026-11-01 pass read
(`~/Armory/VSTs/Kontakt1`). The product folder is the directory above the
nearest `Instruments` ancestor below the root, which is how the census, the
document and the per-library table all name a library. A loose tree — instruments
under `#Non NICNT/<publisher>/<product>/` with no `Instruments` level at all —
has no such marker, so its first directory under the root is used; that is the
same answer the fixed index gave on the 2026-11-01 corpus, and it is a property
of the path rather than of this machine's directory depth.
"""
import os

#: The directory the `.nki` files live under. Nothing in the path arithmetic
#: below assumes its depth.
ROOT = os.environ.get("LIBRARY_ROOT", os.path.expanduser("~/Armory/VSTs/Kontakt1")).rstrip("/")


def _split(path):
    return path.rstrip("/").split("/")


def under_root(path):
    """Whether `path` is inside the library root (not merely equal to it)."""
    parts, root = _split(path), _split(ROOT)
    return len(parts) > len(root) and parts[: len(root)] == root


def library_dir(path):
    """The product folder `path` belongs to, as a path."""
    parts, root = _split(path), _split(ROOT)
    under = len(parts) > len(root) and parts[: len(root)] == root
    # The first component that could be a product folder: the one below the
    # root, or the one below `/` for a path outside the root.
    first = len(root) if under else 1
    if under and first >= len(parts) - 1:
        # A file directly in the root: the root is all the structure there is.
        return ROOT
    if under and parts[first].lower() == "instruments":
        # `Instruments/` sits directly under the root: the root is the product.
        return ROOT
    for i in range(len(parts) - 2, first - 1, -1):
        if parts[i].lower() == "instruments" and i > first:
            return "/".join(parts[:i])
    if under:
        return "/".join(parts[: first + 1])
    return os.path.dirname(path.rstrip("/")) or path


def library_of(path):
    """The product folder's name — what the tables and the document call a library."""
    return os.path.basename(library_dir(path)) or os.path.basename(ROOT)
