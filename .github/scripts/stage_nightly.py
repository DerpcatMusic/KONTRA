#!/usr/bin/env python3
"""Stage Linux and Windows plugin downloads with installation instructions."""
import argparse
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[2]


def stage(platform, output, bundles):
    if platform not in ('linux-x86_64', 'windows-x86_64'):
        raise ValueError('Unsupported platform')
    output.mkdir(parents=True, exist_ok=False)
    for name in ('KONTRA.clap', 'KONTRA.vst3'):
        source = bundles / name
        if source.is_dir():
            shutil.copytree(source, output / name)
        else:
            shutil.copy2(source, output / name)
    windows = platform.startswith('windows')
    extension = 'ps1' if windows else 'sh'
    for name in ('install', 'uninstall'):
        shutil.copy2(ROOT / '.github/packaging' / f'{name}.{extension}', output)
    install = 'powershell -ExecutionPolicy Bypass -File .\\install.ps1' if windows else 'bash ./install.sh'
    uninstall = 'powershell -ExecutionPolicy Bypass -File .\\uninstall.ps1' if windows else 'bash ./uninstall.sh'
    locations = '%LOCALAPPDATA%\\Programs\\Common\\CLAP and VST3' if windows else '~/.clap and ~/.vst3'
    manual_uninstall = (
        'The uninstaller removes per-user copies only. If you copied bundles system-wide,\n'
        'delete %CommonProgramFiles%\\CLAP\\KONTRA.clap and the entire\n'
        '%CommonProgramFiles%\\VST3\\KONTRA.vst3 folder, if present.\n'
        'Windows may ask for administrator permission.\n'
    ) if windows else ''
    if windows:
        scan = ('Use a 64-bit DAW. If it does not scan the per-user locations, add those folders to\n'
                'its scan paths or copy each bundle to its system-wide folder under %CommonProgramFiles%.\n')
    else:
        scan = ('Requires a 64-bit Linux DAW and glibc 2.35 or later. On Debian and Ubuntu,\n'
                'install libxkbcommon0 if the DAW scanner reports a missing libxkbcommon.so.0.\n')
    (output / 'README.txt').write_text(f'KONTRA\n\nClose your DAW and extract the entire archive.\nInstall: {install}\nUninstall: {uninstall}\n{manual_uninstall}Installed per user in {locations}. Rescan plugins in your DAW.\n{scan}Your libraries and settings are preserved when uninstalling.\n\nLicenses: LICENSES.txt.\n', encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform', required=True)
    parser.add_argument('--stage', type=Path, required=True)
    parser.add_argument('--bundles', type=Path, required=True)
    args = parser.parse_args()
    stage(args.platform, args.stage, args.bundles)
