#!/usr/bin/env python3
"""Offline release safety check: python3 .github/workflows/check_nightly.py."""
import json
import hashlib
import os
import plistlib
from pathlib import Path
import subprocess
import tempfile
import textwrap
import zipfile
import base64
from release_notes import generate, render

# A release needs reviewed deltas and full shipped context even without Git history.
previous_notes = '## 0.3.0 — unreleased\n### Added\n- Existing wrapped\n  feature.\n### Fixed\n- Old fix.\n'
current_notes = previous_notes.replace('Existing wrapped\n  feature.', 'Existing wrapped feature.') + '\n- New fix.\n### Changed\n- New behavior.\n### Known limits\n- Runtime limitation remains.\n'
current_notes += '\n### Reviewed source changes\n> Exact authored source message.\n### Candidates — not shipped\n- Untested future feature.\n'
commit = dict(sha='a'*40, commit=dict(message='fix: complete title\n\nExact explanation.'))
followup = dict(sha='d'*40, commit=dict(message='test: retain complete validation context'))
merged = dict(number=13, title='Reviewed batch', html_url='https://example/13', body='All reviewed details.', merged_at='2026-10-02', merge_commit_sha='a'*40)
previous = dict(target_commitish='b'*40)
def notes_api(path, *args):
    if path.startswith('contents/'): return dict(content=base64.b64encode(previous_notes.encode()).decode())
    if path.startswith('compare/'): return [dict(status='ahead', total_commits=2, commits=[commit]), dict(commits=[followup])]
    if '/pulls?' in path: return [[merged, dict(merged, number=14, merged_at=None), dict(merged, number=15, merge_commit_sha='c'*40)]]
    return commit
notes = generate(notes_api, 'example/KONTRA', 'a'*40, '0.3.1-nightly.test', previous, current_notes)
assert 'New fix.' in notes and 'New behavior.' in notes and 'Runtime limitation remains.' in notes
assert '- Existing wrapped feature.' not in notes and '- Old fix.' not in notes
assert 'Exact explanation.' in notes and 'All reviewed details.' in notes and 'Complete public comparison' in notes
assert notes.count('#### [Reviewed batch]') == 1
assert 'Exact authored source message.' in notes and 'Untested future feature.' not in notes
assert 'test: retain complete validation context' in notes
bootstrap = generate(notes_api, 'example/KONTRA', 'a'*40, '0.3.1-nightly.test', None, current_notes)
assert 'First published snapshot' in bootstrap and 'Existing wrapped feature.' in bootstrap
def missing_old_notes(path, *args):
    if path.startswith('contents/'):
        raise subprocess.CalledProcessError(1, ['gh'], stderr=b'gh: Not Found (HTTP 404)')
    return notes_api(path, *args)
assert 'previous source has no changelog' in generate(missing_old_notes, 'example/KONTRA', 'a'*40, '0.3.1-nightly.test', previous, current_notes)
try:
    render('example/KONTRA', 'a'*40, '0.3.1', 'a'*40, '', '', None, [commit], [])
except AssertionError:
    pass
else:
    raise AssertionError('Empty reviewed notes accepted')
try:
    render('example/KONTRA', 'a'*40, '0.3.1', 'a'*40, '## 0.3.1 — date\nWrong checkpoint', '', None, [commit], [])
except AssertionError:
    pass
else:
    raise AssertionError('Wrong frozen checkpoint accepted')

workflow = Path(__file__).with_name("nightly.yml").read_text()
publish_step, cleanup_step = workflow.split("      - name: Publish the experimental nightly\n", 1)[1].split("      - name: Remove released Actions artifacts\n", 1)
publish = "python3 " + str(Path(__file__).with_name("publish_nightly.py").resolve())
cleanup = textwrap.dedent(cleanup_step.split("        run: |\n", 1)[1])
platforms = ("linux-x86_64", "windows-x86_64", "macos-arm64", "macos-x86_64")
readme = Path(__file__).resolve().parents[2].joinpath("README.md").read_text()
for platform in platforms:
    assert f"name: {platform}" in workflow
    assert f"releases/latest/download/KONTRA-nightly-{platform}.zip" in readme
assert "nightly-build-${{ matrix.name }}\n      cancel-in-progress: true" in workflow
assert "nightly-publish\n      cancel-in-progress: false" in workflow
assert "retention-days: 1" in workflow
assert "if: success()" in cleanup_step
assert "continue-on-error: true" in cleanup_step

# Exercise the workflow's actual selector against per-format and stale outputs.
selector = textwrap.dedent(workflow.split("          python3 - <<'PYINFO'\n", 1)[1].split("          PYINFO", 1)[0]).replace("${{ matrix.target }}", "x86_64-unknown-linux-gnu")
plist_step = textwrap.dedent(workflow.split("          python3 - <<'PYPLIST'\n", 1)[1].split("          PYPLIST", 1)[0])
for case in ("valid", "missing", "duplicate"):
    with tempfile.TemporaryDirectory(prefix="kontra-format-check-") as directory:
        root = Path(directory)
        version = "0.2.0-nightly.20261001.gaaaaaaaaaaaa"
        root.joinpath("Cargo.toml").write_text('[package]\nversion="'+version+'"\n')
        env = dict(os.environ, GITHUB_SHA="a"*40, STAGE="stage")
        for format in ("clap", "vst3", "standalone", "stale", "wrong-target", "wrong-profile", "duplicate"):
            if format == "clap" and case == "missing" or format == "duplicate" and case != "duplicate": continue
            info = dict(version=version, revision="a"*40, target="x86_64-unknown-linux-gnu", profile="release", features=["plugin","library-access", "clap" if format in ("stale","wrong-target","wrong-profile","duplicate") else format])
            if format == "stale": info["revision"] = "b"*40
            if format == "wrong-target": info["target"] = "aarch64-apple-darwin"
            if format == "wrong-profile": info["profile"] = "debug"
            path = root / f"target/x86_64-unknown-linux-gnu/release/build/kontakto-{format}/out/kontra-build.json"
            path.parent.mkdir(parents=True); path.write_text(json.dumps(info))
        result = subprocess.run(["python3", "-c", selector], cwd=root, env=env, capture_output=True)
        assert result.returncode == (0 if case == "valid" else 1), (case, result.stderr)
        if case == "valid":
            for format in ("clap", "vst3"):
                info=json.loads(root.joinpath(format+"-build-info.json").read_text())
                assert set(info["features"]) & {"clap","vst3","standalone"} == {format}
                path=root / f"stage/KONTRA.{format}/Contents/Info.plist"
                path.parent.mkdir(parents=True); path.write_bytes(plistlib.dumps(dict(CFBundleVersion="1",CFBundleIdentifier="preserved")))
            result=subprocess.run(["python3", "-c", plist_step], cwd=root, env=env, capture_output=True)
            assert result.returncode == 0, result.stderr
            for format in ("clap", "vst3"):
                info=plistlib.loads(root.joinpath(f"stage/KONTRA.{format}/Contents/Info.plist").read_bytes())
                assert info["CFBundleShortVersionString"] == info["CFBundleVersion"] == "0.2.0"
                assert info["KONTRAVersion"] == version and info["CFBundleIdentifier"] == "preserved"

mock_gh = r'''#!/usr/bin/env python3
import base64,hashlib,json,os,sys
from pathlib import Path
p=Path("state.json"); s=json.loads(p.read_text()); a=sys.argv[1:]; s["calls"].append(a)
out=""; status=0
sha=os.environ["GITHUB_SHA"]
def arg(key): return a[a.index(key)+1]
def save(): p.write_text(json.dumps(s))
if a[0]=="api":
    path=next(v for v in a if v.startswith("repos/example/KONTRA/")).split("repos/example/KONTRA/",1)[1]
    if path=="releases": out=json.dumps([s["releases"]])
    elif path.startswith("contents/CHANGELOG.md?"): out=json.dumps({"content":base64.b64encode("## Previous — unreleased\n### Added\n- Previous feature.\n".encode()).decode()})
    elif path.startswith("compare/"): out=json.dumps([{"status":"ahead","total_commits":1,"commits":[{"sha":sha,"commit":{"message":"fix: reviewed snapshot\n\nComplete shipped detail."}}]}])
    elif path.startswith("commits/") and "/pulls?" in path: out="[[]]"
    elif path.startswith("commits/"): out=json.dumps({"sha":sha,"commit":{"message":"fix: reviewed snapshot\n\nComplete shipped detail."}})
    elif path=="releases/latest": out=json.dumps(next(r for r in s["releases"] if r["id"]==s["latest"]))
    elif path=="git/ref/heads/main":
        s["heads"]+=1
        stale=s["case"]=="stale-before" or (s["case"]=="stale-after" and s["heads"]==2)
        out=json.dumps({"object":{"sha":"e"*40 if stale else sha}})
    elif path.startswith("git/matching-refs/tags/"):
        tag=path.split("tags/",1)[1]
        out=json.dumps([{"ref":"refs/tags/"+t,"object":{"sha":v}} for t,v in s["refs"].items() if t.startswith(tag)])
    elif path=="git/refs":
        values={v.split("=",1)[0]:v.split("=",1)[1] for i,v in enumerate(a) if i and a[i-1]=="-f"}
        assert values["sha"]==sha, "GITHUB_TOKEN cannot create historical workflow refs"
        s["refs"][values["ref"].removeprefix("refs/tags/")]=values["sha"]; out="{}"
    elif path.startswith("git/refs/tags/"):
        assert arg("--method")=="DELETE"
        tag=path.split("tags/",1)[1]
        if tag not in s["refs"]:
            save();print("gh: Reference does not exist (HTTP 422)",file=sys.stderr);sys.exit(1)
        s["refs"].pop(tag); out="{}"
    elif "/actions/runs/" in path or path.startswith("actions/runs/"):
        assert (s["promoted"] or s["case"] in ("stale-before","stale-after")) and path=="actions/runs/7/artifacts"
        out="100\n101\n102\n103"
    elif path.startswith("actions/artifacts/"):
        assert (s["promoted"] or s["case"] in ("stale-before","stale-after")) and arg("--method")=="DELETE"
        s["deleted"].append(int(path.rsplit("/",1)[1])); status=int(s["case"]=="cleanup-fails"); out="{}"
    else: raise AssertionError(a)
elif a[:2]==["release","create"]:
    assert a[2]=="v"+os.environ["TEST_VERSION"] and "--draft" in a and "--prerelease" not in a
    assert len([r for r in s["releases"] if not r["draft"]]) in (0,2,3)
    s["published"]=False
    assets=[]
    for file in a:
        if file.startswith("dist/"):
            data=Path(file).read_bytes(); assets.append(dict(name=Path(file).name,size=len(data),digest="sha256:"+hashlib.sha256(data).hexdigest(),state="uploaded"))
    release_id=max((r["id"] for r in s["releases"]),default=0)+1
    s["releases"].append(dict(id=release_id,tag_name=a[2],name=arg("--title"),target_commitish=arg("--target"),draft=True,prerelease=False,assets=assets,body=Path("notes.md").read_text(),published_at="2026-10-02"))
    if s["case"]=="bad-digest": assets[0]["digest"]="sha256:invalid"
    s["manifests"][str(release_id)]=Path("dist/release-manifest.json").read_text(); status=int(s["case"]=="upload-fails")
elif a[:2]==["release","download"]:
    r=next(r for r in s["releases"] if r["tag_name"]==a[2]); out=s["manifests"][str(r["id"])]
elif a[:2]==["release","delete"]:
    r=next(r for r in s["releases"] if r["tag_name"]==a[2])
    if not r["draft"]:
        assert s["published"], "rollback deleted before verified publication"
        if s["case"]=="rotation-fails" and not s.get("rotation_failed"):
            s["rotation_failed"]=True;save();sys.exit(1)
    s["releases"].remove(r)
    if "--cleanup-tag" in a: s["refs"].pop(a[2],None)
elif a[:2]==["release","edit"]:
    r=next(r for r in s["releases"] if r["tag_name"]==a[2])
    assert r["target_commitish"]==sha, "GITHUB_TOKEN cannot edit historical workflow releases"
    assert "--prerelease=false" in a and "--latest=true" in a
    if "--draft=false" in a:
        assert s["heads"]>=2
        r["draft"]=False; s["published"]=True
        s["refs"][r["tag_name"]]=r["target_commitish"]
    if "--tag" in a:
        assert a[2]=="nightly-staging" and s["published"]
        tag=arg("--tag")
        assert not any(other["tag_name"]==tag for other in s["releases"] if other is not r)
        r["tag_name"]=tag;s["refs"][tag]=arg("--target")
    r["prerelease"]=False;s["latest"]=r["id"];s["promoted"]=True
else: raise AssertionError(a)
save(); print(out,end="" if a[:2]==["release","download"] else "\n"); sys.exit(status)
'''

cases=("current","first","bad-digest","stale-before","stale-after","upload-fails","missing-asset","bad-checksum","wrong-format","cleanup-fails","rotation-fails")
for case in cases:
    with tempfile.TemporaryDirectory(prefix="kontra-nightly-check-") as directory:
        root=Path(directory); root.joinpath("gh").write_text(mock_gh); root.joinpath("gh").chmod(0o755); root.joinpath("dist").mkdir()
        version="0.2.0-nightly.20261002.gaaaaaaaaaaaa"
        root.joinpath("Cargo.toml").write_text('[package]\nversion="'+version+'"\n')
        root.joinpath("CHANGELOG.md").write_text('## Current — unreleased\n### Added\n- Reviewed source feature.\n### Fixed\n- Reviewed defect corrected.\n### Known limits\n- Hosted compilation does not prove DAW compatibility.\n')
        for platform in platforms:
            if case=="missing-asset" and platform==platforms[-1]: continue
            with zipfile.ZipFile(root/f"dist/KONTRA-nightly-{platform}.zip","w") as z:
                for name in ("build-info.json","clap-build-info.json","vst3-build-info.json"):
                    target={"linux-x86_64":"x86_64-unknown-linux-gnu","windows-x86_64":"x86_64-pc-windows-msvc","macos-arm64":"aarch64-apple-darwin","macos-x86_64":"x86_64-apple-darwin"}[platform]
                    z.writestr(f"KONTRA-nightly-{platform}/{name}",json.dumps(dict(version=version,revision="a"*40,target=target,profile="release",features=["plugin","library-access"]+(["clap","vst3","standalone"] if name=="build-info.json" else [name.split("-")[0]])+(["vst3"] if case=="wrong-format" and name=="clap-build-info.json" else []))))
                binaries=("KONTRA.clap/Contents/MacOS/KONTRA","KONTRA.vst3/Contents/MacOS/KONTRA","kontakto-standalone") if platform.startswith("macos-") else (("KONTRA.clap","KONTRA.vst3/Contents/x86_64-win/KONTRA.vst3","kontakto-standalone.exe") if platform.startswith("windows-") else ("KONTRA.clap","KONTRA.vst3/Contents/x86_64-linux/KONTRA.so","kontakto-standalone"))
                if platform.startswith("macos-"):
                    for bundle in ("KONTRA.clap", "KONTRA.vst3"):
                        z.writestr(f"KONTRA-nightly-{platform}/{bundle}/Contents/Info.plist", plistlib.dumps(dict(CFBundleShortVersionString="0.2.0",CFBundleVersion="0.2.0",KONTRAVersion=version)))
                z.writestr(f"KONTRA-nightly-{platform}/SOURCE_COMMIT.txt", "a"*40+"\n")
                for name in (*binaries,"LICENSE","NOTICE","THIRD_PARTY.md"): z.writestr(f"KONTRA-nightly-{platform}/{name}",b"fixture")
            archive=root/f"dist/KONTRA-nightly-{platform}.zip"
            digest=hashlib.sha256(archive.read_bytes()).hexdigest()
            archive.with_suffix(".zip.sha256").write_text(("0"*64 if case=="bad-checksum" else digest)+"  "+archive.name+"\n")
        old=[]; refs={}
        if case!="first":
            for i,tag in enumerate(("nightly","nightly-previous","v1.0.0"),1):
                oldver=f"v0.1.0-nightly.2026090{i}.g{chr(97+i)*12}"
                source=chr(97+i)*40
                old.append(dict(id=i,tag_name=tag,target_commitish=source,draft=False,prerelease=True,name="legacy",assets=[dict(name=f"KONTRA-nightly-{platform}.zip",state="uploaded",size=7,digest="sha256:"+hashlib.sha256(b"fixture").hexdigest()) for platform in platforms],body=f"<!-- kontra-source-tag: {oldver} -->" if i!=1 else "Legacy automated snapshot",published_at=f"2026-09-0{4-i}"))
                refs[tag]=source
                if i!=1: refs[oldver]=source
        if case=="current":
            old.append(dict(id=5,tag_name="nightly-staging",draft=True))
            orphan="v0.2.0-nightly.20261001.g777777777777"
            old.append(dict(id=6,tag_name=orphan,draft=True));refs[orphan]="7"*40
        refs["nightly-staging-other"]="e"*40
        initial=dict(case=case,releases=old,refs=refs,heads=0,published=False,promoted=False,latest=None,calls=[],deleted=[],manifests={})
        root.joinpath("state.json").write_text(json.dumps(initial)); output=root/"outputs"; output.touch()
        env=dict(os.environ,PATH=f"{root}:{os.environ['PATH']}",GITHUB_SHA="a"*40,GH_REPO="example/KONTRA",GITHUB_RUN_ID="7",GITHUB_OUTPUT=str(output),TEST_VERSION=version)
        def run(command): return subprocess.run(["bash","--noprofile","--norc","-e","-o","pipefail","-c",command],cwd=root,env=env,capture_output=True,text=True)
        result=run(publish); state=json.loads(root.joinpath("state.json").read_text())
        assert result.returncode==(1 if case in ("upload-fails","missing-asset","rotation-fails","bad-digest","bad-checksum","wrong-format") else 0),(case,result.stderr)
        if case=="rotation-fails":
            assert state["published"] and len(state["releases"])==4 and state["latest"] is not None
            result=run(publish); assert result.returncode==0,result.stderr
            state=json.loads(root.joinpath("state.json").read_text())
        promoted=case in ("current","first","cleanup-fails","rotation-fails")
        assert state["promoted"]==promoted,(case,state)
        assert state["refs"]["nightly-staging-other"]=="e"*40
        assert not any(r["draft"] for r in state["releases"]),(case,state)
        if promoted:
            assert len(state["releases"])==(1 if case=="first" else 2)
            newest=next(r for r in state["releases"] if r["tag_name"]=="v"+version)
            assert newest["target_commitish"]=="a"*40 and newest["name"]=="KONTRA "+version
            assert state["refs"]["v"+version]=="a"*40 and not newest["prerelease"] and state["latest"]==newest["id"]
            if case!="first":
                assert state["refs"]["nightly"]=="b"*40
                assert next(r for r in state["releases"] if r["id"]==1)==initial["releases"][0], "Previous release must remain unchanged"
                assert state["refs"]["v1.0.0"]=="d"*40
                assert not any(r["tag_name"]=="v1.0.0" for r in state["releases"])
            assert len([tag for tag in state["refs"] if tag.startswith(("v","legacy-g")) and tag!="v1.0.0"])==1
            if case=="current":
                # Recover the real legacy staging failure, using its existing source tag.
                newest["tag_name"]="nightly-staging";newest["prerelease"]=True
                state["refs"]["nightly-staging"]="a"*40;state["promoted"]=False
                state["latest"]=None;root.joinpath("state.json").write_text(json.dumps(state))
                env["GITHUB_SHA"]="e"*40
                result=run(publish);assert result.returncode==1,result.stderr
                assert json.loads(root.joinpath("state.json").read_text())["releases"]==state["releases"], "Historical staging recovery must fail without changes"
                env["GITHUB_SHA"]="a"*40
                result=run(publish);assert result.returncode==0,result.stderr
                state=json.loads(root.joinpath("state.json").read_text())
                assert len(state["releases"])==2 and state["latest"]==newest["id"]
            # A rerun must leave the same rollback and published release identities.
            before=state["releases"]
            result=run(publish); assert result.returncode==0,result.stderr
            state=json.loads(root.joinpath("state.json").read_text()); assert state["releases"]==before
            if case=="current":
                # The following snapshot must verify the previous versioned manifest.
                env["GITHUB_SHA"]="f"*40
                next_version="0.2.0-nightly.20261002.gffffffffffff"
                env["TEST_VERSION"]=next_version
                root.joinpath("Cargo.toml").write_text('[package]\nversion="'+next_version+'"\n')
                for platform in platforms:
                    path=root/f"dist/KONTRA-nightly-{platform}.zip"
                    with zipfile.ZipFile(path) as z: entries={n:z.read(n) for n in z.namelist()}
                    with zipfile.ZipFile(path,"w") as z:
                        for name,data in entries.items():
                            if name.endswith("build-info.json"):
                                info=json.loads(data);info.update(version=next_version,revision="f"*40);data=json.dumps(info).encode()
                            elif name.endswith("Info.plist"):
                                plist=plistlib.loads(data);plist["KONTRAVersion"]=next_version;data=plistlib.dumps(plist)
                            elif name.endswith("SOURCE_COMMIT.txt"):
                                data=("f"*40+"\n").encode()
                            z.writestr(name,data)
                    path.with_suffix(".zip.sha256").write_text(hashlib.sha256(path.read_bytes()).hexdigest()+"  "+path.name+"\n")
                result=run(publish);assert result.returncode==0,result.stderr
                state=json.loads(root.joinpath("state.json").read_text())
                assert len(state["releases"])==2
                assert state["refs"]["v"+next_version]=="f"*40 and state["refs"]["v"+version]=="a"*40
                assert "nightly" not in state["refs"]
                assert {r["tag_name"] for r in state["releases"]}=={"v"+version,"v"+next_version}
                assert "legacy-g"+"b"*12 not in state["refs"]
                assert state["refs"]["v1.0.0"]=="d"*40
                before=state["releases"]
            result=run(cleanup); state=json.loads(root.joinpath("state.json").read_text())
            assert result.returncode==(1 if case=="cleanup-fails" else 0),(case,result.stderr)
            assert state["deleted"]==([100] if case=="cleanup-fails" else [100,101,102,103])
            assert state["releases"]==before
        else:
            assert len(state["releases"])==3 and not state["deleted"]
            assert state["refs"]["nightly"]=="b"*40 and state["refs"]["nightly-previous"]=="c"*40
            if case in ("stale-before","stale-after"):
                before=state["releases"]
                result=run(cleanup);assert result.returncode==0,result.stderr
                state=json.loads(root.joinpath("state.json").read_text())
                assert state["deleted"]==[100,101,102,103] and state["releases"]==before
        assert ("published=true" in output.read_text())==promoted,case
print("Nightly checks passed: reviewed delta/history/bootstrap notes, format selection/plist checks, 11 retention/rerun/upload/checksum/cleanup scenarios and four stable README links.")
