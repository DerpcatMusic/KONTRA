#!/usr/bin/env python3
"""POSIX-only developer process limits, not an embedded plugin runtime."""
import os
from pathlib import Path
import resource
import sys

resource.setrlimit(resource.RLIMIT_AS, (256 * 1024 * 1024, 256 * 1024 * 1024))
resource.setrlimit(resource.RLIMIT_CPU, (2, 2))
resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
probe = Path(__file__).with_name("probe_native_ui_scene.lua")
os.execvp("lua5.4", ["lua5.4", str(probe), *sys.argv[1:]])
