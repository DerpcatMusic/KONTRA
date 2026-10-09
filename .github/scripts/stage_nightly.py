#!/usr/bin/env python3
"""Stage lean Linux/Windows downloads; identity and MPL source stay outside the ZIP."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import zipfile

ROOT = Path(__file__).resolve().parents[2]
FORMATS = ('clap','vst3','standalone')
TARGETS = {'linux-x86_64':'x86_64-unknown-linux-gnu','windows-x86_64':'x86_64-pc-windows-msvc'}
LEGAL = ('LICENSE','NOTICE','THIRD_PARTY.md','docs/LEGAL.md','assets/OFL.txt',
         'crates/sampler-uvi/assets/assistant/OFL.txt','crates/sampler-kontakt/FASTLZ_NOTICE.txt')
BUNDLE_LEGAL = ('THIRD_PARTY_NOTICES.txt','MUI/LICENSE','MOOSE/LICENSE','MOOSE/LICENSE-MIT','MOOSE/LICENSE-APACHE','MOOSE/NOTICE','BUFFR/LICENSE','BUFFR/SOURCE.md')
SOURCES_ASSET = 'KONTRA-nightly-covered-source.zip'


def digest(data):return hashlib.sha256(data).hexdigest()


def stage(platform, output, bundles, standalone, licenses, builds, revision, source=ROOT):
    assert platform in TARGETS and re.fullmatch('[0-9a-f]{40}',revision)
    assert not output.exists(),'Refuse to overwrite a stage'
    assert set(builds)==set(FORMATS)
    version=builds['standalone']['version']
    for fmt, info in builds.items():
        assert info['revision']==revision and info['version']==version and info['target']==TARGETS[platform] and info['profile']=='release'
        features=set(info['features'])
        assert {'plugin','library-access',fmt}<=features
        assert features & set(FORMATS)==({fmt} if fmt!='standalone' else set(FORMATS))
    texts={name:(source/name).read_text(encoding='utf-8') for name in LEGAL}
    for name in BUNDLE_LEGAL:assert (licenses/name).is_file() and (licenses/name).stat().st_size,name
    for path in sorted(licenses.rglob('*')):
        if path.is_file() and 'sources' not in path.relative_to(licenses).parts:
            texts['licenses/'+path.relative_to(licenses).as_posix()]=path.read_text(encoding='utf-8')
    assert all(text.strip() for text in texts.values())
    covered=re.findall(r'^(\S+) (\S+): .*MPL-2\.0.*$',texts['licenses/THIRD_PARTY_NOTICES.txt'],re.M)
    assert covered,'Missing MPL source inventory'
    sources={f'{name}-{version}.crate':(licenses/'sources'/f'{name}-{version}.crate').read_bytes() for name,version in covered}
    assert all(sources.values()),'Empty covered source'
    exe='kontakto-standalone.exe' if platform.startswith('windows-') else 'kontakto-standalone'
    binary_names=('KONTRA.clap','KONTRA.vst3/Contents/x86_64-win/KONTRA.vst3',exe) if platform.startswith('windows-') else ('KONTRA.clap','KONTRA.vst3/Contents/x86_64-linux/KONTRA.so',exe)
    assert all((bundles/name).is_file() for name in binary_names[:2]) and standalone.is_file()
    output.mkdir(parents=True)
    for name in ('KONTRA.clap','KONTRA.vst3'):
        path=bundles/name
        if path.is_dir():shutil.copytree(path,output/name)
        else:shutil.copy2(path,output/name)
    shutil.copy2(standalone,output/exe)
    license_text='KONTRA redistribution notices and complete license texts\nOriginal source paths below identify sections of this consolidated file.\nMPL covered source (including our patched code) is in '+SOURCES_ASSET+' on the same release.\n\n'
    license_text+='\n\n'.join('===== '+name+' =====\n'+text for name,text in texts.items())+'\n'
    (output/'LICENSES.txt').write_text(license_text,encoding='utf-8')
    windows=platform.startswith('windows-')
    script_ext='ps1' if windows else 'sh'
    for script in ('install','uninstall'):
        path=output/(script+'.'+script_ext);shutil.copyfile(source/'.github/packaging'/path.name,path);path.chmod(0o755)
    install='powershell -ExecutionPolicy Bypass -File .\\install.ps1' if windows else 'bash ./install.sh'
    uninstall='powershell -ExecutionPolicy Bypass -File .\\uninstall.ps1' if windows else 'bash ./uninstall.sh'
    locations=('%LOCALAPPDATA%\\Programs\\Common\\CLAP and VST3; standalone: %LOCALAPPDATA%\\Programs\\KONTRA' if windows else '~/.clap and ~/.vst3; standalone: ~/.local/bin/kontakto-standalone')
    logs='%LOCALAPPDATA%\\kontra\\logs' if windows else '${XDG_CACHE_HOME:-~/.cache}/kontra/logs'
    (output/'README.txt').write_text(f'KONTRA {version}\nA sampler for Kontakt and Falcon/UVI libraries. No instrument libraries are included.\n\nInstall: close your DAW, extract this entire archive, then run:\n{install}\nPer-user locations: {locations}\nRescan plugins in your DAW after installation.\n\nUninstall: close your DAW and run:\n{uninstall}\nThis removes the installed files, preserving your libraries, settings and logs.\n\nLogs: {logs} (KONTRA_LOG_DIR overrides this location).\n\nInstall help and system requirements:\nhttps://github.com/DerpcatMusic/KONTRA/blob/v{version}/README.md\n\nLicenses and existing redistribution notices: LICENSES.txt.\nCovered MPL source: {SOURCES_ASSET}, available on the same GitHub release.\n',encoding='utf-8')
    if not windows:
        with zipfile.ZipFile(output.parent/SOURCES_ASSET,'w',zipfile.ZIP_DEFLATED) as z:
            for name,data in sources.items():z.writestr('sources/'+name,data)
    info=dict(revision=revision,version=version,platform=platform,builds=builds,
              files={name:digest((output/name).read_bytes()) for name in (*binary_names,'LICENSES.txt','README.txt','install.'+script_ext,'uninstall.'+script_ext)},
              license_sections=list(texts),covered_sources={name:digest(data) for name,data in sources.items()})
    output.with_suffix('.build.json').write_text(json.dumps(info,indent=2)+'\n')
    return info


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--platform',choices=TARGETS,required=True);p.add_argument('--stage',type=Path,required=True)
    p.add_argument('--bundles',type=Path,required=True);p.add_argument('--standalone',type=Path,required=True)
    p.add_argument('--licenses',type=Path,required=True);p.add_argument('--revision',required=True)
    p.add_argument('--clap-info',type=Path,required=True);p.add_argument('--vst3-info',type=Path,required=True);p.add_argument('--standalone-info',type=Path,required=True)
    a=p.parse_args();info={fmt:json.loads(getattr(a,fmt+'_info').read_text()) for fmt in FORMATS}
    stage(a.platform,a.stage,a.bundles,a.standalone,a.licenses,info,a.revision)
