#!/usr/bin/env python3
"""Check that native CLAP and VST3 bundles load and expose their entry points."""

import argparse
import ctypes
import os
from pathlib import Path
import re
import subprocess


def binaries(platform, bundles, installed=False):
    vst3 = {
        "linux-x86_64": "Contents/x86_64-linux/KONTRA.so",
        "windows-x86_64": "Contents/x86_64-win/KONTRA.vst3",
    }[platform]
    clap_root = bundles / "CLAP" if installed else bundles
    vst3_root = bundles / "VST3" if installed else bundles
    return [(clap_root / "KONTRA.clap", "clap_entry"),
            (vst3_root / "KONTRA.vst3" / vst3, "GetPluginFactory")]


def find_dumpbin():
    vswhere = Path(os.environ["ProgramFiles(x86)"]) / "Microsoft Visual Studio/Installer/vswhere.exe"
    found = subprocess.check_output([
        vswhere, "-latest", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-find", r"VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe",
    ], text=True).splitlines()
    dumpbin = next(iter(found), None)
    if not dumpbin:
        raise ValueError("could not locate the MSVC x64 dumpbin utility")
    return dumpbin


def check_windows_imports(report):
    # dumpbin indents the imported module names in both import tables.
    imports = re.findall(r"^\s+([\w.-]+\.dll)\s*$", report, re.IGNORECASE | re.MULTILINE)
    if not imports:
        raise ValueError("dumpbin found no DLL imports")
    print("Imports:", ", ".join(imports))
    redistributable = re.compile(
        r"(?:vcruntime|msvcp|msvcr|concrt|vccorlib)\d+.*\.dll",
        re.IGNORECASE,
    )
    crt = sorted({dll.lower() for dll in imports if redistributable.fullmatch(dll)})
    if crt:
        raise ValueError(f"dynamic MSVC runtime imports: {', '.join(crt)}")


def check_binary(path, entry, dumpbin=None):
    if not path.is_file():
        raise ValueError(f"missing plugin binary: {path}")
    print(f"Checking {path}", flush=True)
    if dumpbin:
        check_windows_imports(subprocess.check_output([dumpbin, "/imports", path], text=True))
        # Use the default Windows loader search flags, like a DAW plugin scan.
        image = ctypes.WinDLL(str(path.resolve()), winmode=0)
    else:
        dynamic = subprocess.check_output(["readelf", "--wide", "--dynamic", path], text=True)
        print("\n".join(line.strip() for line in dynamic.splitlines() if "(NEEDED)" in line))
        image = ctypes.CDLL(str(path.resolve()))
    getattr(image, entry)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=("linux-x86_64", "windows-x86_64"), required=True)
    parser.add_argument("--bundles", type=Path, required=True)
    parser.add_argument("--installed", action="store_true", help="check the Windows installer layout")
    args = parser.parse_args()
    windows = args.platform == "windows-x86_64"
    if args.installed and not windows:
        parser.error("--installed is supported for Windows only")
    try:
        dumpbin = find_dumpbin() if windows else None
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.error(str(error))
    for path, entry in binaries(args.platform, args.bundles, args.installed):
        try:
            check_binary(path, entry, dumpbin)
        except (OSError, ValueError, AttributeError, subprocess.CalledProcessError) as error:
            parser.error(f"{path}: {error}")


if __name__ == "__main__":
    main()
