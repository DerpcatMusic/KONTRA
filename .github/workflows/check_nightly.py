#!/usr/bin/env python3
"""Offline release safety check: python3 .github/workflows/check_nightly.py."""
import json
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import zipfile

workflow = Path(__file__).with_name("nightly.yml").read_text()
publish_step, cleanup_step = workflow.split("      - name: Publish the nightly pre-release\n", 1)[1].split("      - name: Remove released Actions artifacts\n", 1)
publish = "python3 " + str(Path(__file__).with_name("publish_nightly.py").resolve())
cleanup = textwrap.dedent(cleanup_step.split("        run: |\n", 1)[1])
platforms = ("linux-x86_64", "windows-x86_64", "macos-arm64", "macos-x86_64")
readme = Path(__file__).resolve().parents[2].joinpath("README.md").read_text()
for platform in platforms:
    assert f"name: {platform}" in workflow
    assert f"releases/download/nightly/KONTRA-nightly-{platform}.zip" in readme
assert "nightly-build-${{ matrix.name }}\n      cancel-in-progress: true" in workflow
assert "nightly-publish\n      cancel-in-progress: false" in workflow
assert "retention-days: 1" in workflow
assert "if: success()" in cleanup_step
assert "continue-on-error: true" in cleanup_step

mock_gh = r'''#!/usr/bin/env python3
import hashlib,json,os,sys
from pathlib import Path
p=Path("state.json"); s=json.loads(p.read_text()); a=sys.argv[1:]; s["calls"].append(a)
out=""; status=0
sha=os.environ["GITHUB_SHA"]
def arg(key): return a[a.index(key)+1]
def save(): p.write_text(json.dumps(s))
if a[0]=="api":
    path=next(v for v in a if v.startswith("repos/example/KONTRA/")).split("repos/example/KONTRA/",1)[1]
    if path=="releases": out=json.dumps([s["releases"]])
    elif path=="git/ref/heads/main":
        s["heads"]+=1
        stale=s["case"]=="stale-before" or (s["case"]=="stale-after" and s["heads"]==2)
        out=json.dumps({"object":{"sha":"e"*40 if stale else sha}})
    elif path.startswith("git/matching-refs/tags/"):
        tag=path.split("tags/",1)[1]
        out=json.dumps([{"ref":"refs/tags/"+t,"object":{"sha":v}} for t,v in s["refs"].items() if t.startswith(tag)])
    elif path=="git/refs":
        values={v.split("=",1)[0]:v.split("=",1)[1] for i,v in enumerate(a) if i and a[i-1]=="-f"}
        s["refs"][values["ref"].removeprefix("refs/tags/")]=values["sha"]; out="{}"
    elif path.startswith("git/refs/tags/"):
        assert arg("--method")=="DELETE"
        s["refs"].pop(path.split("tags/",1)[1],None); out="{}"
    elif "/actions/runs/" in path or path.startswith("actions/runs/"):
        assert (s["promoted"] or s["case"] in ("stale-before","stale-after")) and path=="actions/runs/7/artifacts"
        out="100\n101\n102\n103"
    elif path.startswith("actions/artifacts/"):
        assert (s["promoted"] or s["case"] in ("stale-before","stale-after")) and arg("--method")=="DELETE"
        s["deleted"].append(int(path.rsplit("/",1)[1])); status=int(s["case"]=="cleanup-fails"); out="{}"
    else: raise AssertionError(a)
elif a[:2]==["release","create"]:
    assert a[2]=="nightly-staging" and "--draft" in a and "--prerelease" in a
    assert len([r for r in s["releases"] if not r["draft"]]) in (0,2,3)
    s["published"]=False
    assets=[]
    for file in a:
        if file.startswith("dist/"):
            data=Path(file).read_bytes(); assets.append(dict(name=Path(file).name,size=len(data),digest="sha256:"+hashlib.sha256(data).hexdigest(),state="uploaded"))
    release_id=max((r["id"] for r in s["releases"]),default=0)+1
    s["releases"].append(dict(id=release_id,tag_name="nightly-staging",name=arg("--title"),target_commitish=arg("--target"),draft=True,assets=assets,body=Path("notes.md").read_text(),published_at="2026-10-02"))
    if s["case"]=="bad-digest": assets[0]["digest"]="sha256:invalid"
    s["manifests"][str(release_id)]=Path("dist/release-manifest.json").read_text(); status=int(s["case"]=="upload-fails")
elif a[:2]==["release","download"]:
    r=next(r for r in s["releases"] if r["tag_name"]==a[2]); out=s["manifests"][str(r["id"])]
elif a[:2]==["release","delete"]:
    r=next(r for r in s["releases"] if r["tag_name"]==a[2])
    if not r["draft"]: assert s["published"], "rollback deleted before verified publication"
    s["releases"].remove(r)
    if "--cleanup-tag" in a: s["refs"].pop(a[2],None)
elif a[:2]==["release","edit"]:
    r=next(r for r in s["releases"] if r["tag_name"]==a[2])
    if "--draft=false" in a:
        assert s["heads"]>=2
        r["draft"]=False; s["published"]=True
    else:
        tag=arg("--tag")
        assert s["published"]
        assert not any(other["tag_name"]==tag for other in s["releases"] if other is not r)
        if tag=="nightly" and s["case"]=="rotation-fails" and not s.get("rotation_failed"):
            status=1; s["rotation_failed"]=True
        else:
            r["tag_name"]=tag; s["refs"][tag]=arg("--target")
            if "--title" in a: r["name"]=arg("--title")
            if "--notes-file" in a: r["body"]=Path(arg("--notes-file")).read_text()
            if tag=="nightly": s["promoted"]=True
else: raise AssertionError(a)
save(); print(out,end="" if a[:2]==["release","download"] else "\n"); sys.exit(status)
'''

cases=("current","first","bad-digest","stale-before","stale-after","upload-fails","missing-asset","cleanup-fails","rotation-fails")
for case in cases:
    with tempfile.TemporaryDirectory(prefix="kontra-nightly-check-") as directory:
        root=Path(directory); root.joinpath("gh").write_text(mock_gh); root.joinpath("gh").chmod(0o755); root.joinpath("dist").mkdir()
        version="0.2.0-nightly.20261002.gaaaaaaaaaaaa"
        root.joinpath("Cargo.toml").write_text('[package]\nversion="'+version+'"\n')
        for platform in platforms:
            if case=="missing-asset" and platform==platforms[-1]: continue
            with zipfile.ZipFile(root/f"dist/KONTRA-nightly-{platform}.zip","w") as z:
                for name in ("build-info.json","plugin-build-info.json"):
                    target={"linux-x86_64":"x86_64-unknown-linux-gnu","windows-x86_64":"x86_64-pc-windows-msvc","macos-arm64":"aarch64-apple-darwin","macos-x86_64":"x86_64-apple-darwin"}[platform]
                    z.writestr(f"KONTRA-nightly-{platform}/{name}",json.dumps(dict(version=version,revision="a"*40,target=target,profile="release",features=["library-access"]+(["standalone"] if name=="build-info.json" else []))))
                binaries=("KONTRA.clap/Contents/MacOS/KONTRA","KONTRA.vst3/Contents/MacOS/KONTRA","kontakto-standalone") if platform.startswith("macos-") else (("KONTRA.clap","KONTRA.vst3/Contents/x86_64-win/KONTRA.vst3","kontakto-standalone.exe") if platform.startswith("windows-") else ("KONTRA.clap","KONTRA.vst3/Contents/x86_64-linux/KONTRA.so","kontakto-standalone"))
                for name in (*binaries,"LICENSE","NOTICE","THIRD_PARTY.md"): z.writestr(f"KONTRA-nightly-{platform}/{name}",b"fixture")
        old=[]; refs={}
        if case!="first":
            for i,tag in enumerate(("nightly","nightly-previous","v0.0.1"),1):
                oldver=f"v0.1.0-nightly.2026090{i}.g{chr(97+i)*12}"
                source=chr(97+i)*40
                old.append(dict(id=i,tag_name=tag,target_commitish=source,draft=False,name="legacy",assets=[dict(name=f"KONTRA-nightly-{platform}.zip",state="uploaded",size=7,digest="sha256:"+hashlib.sha256(b"fixture").hexdigest()) for platform in platforms],body=f"<!-- kontra-source-tag: {oldver} -->" if i!=1 else "Legacy automated snapshot",published_at=f"2026-09-0{4-i}"))
                refs[tag]=source
                if i!=1: refs[oldver]=source
        if case=="current": old.append(dict(id=5,tag_name="nightly-staging",draft=True))
        initial=dict(case=case,releases=old,refs=refs,heads=0,published=False,promoted=False,calls=[],deleted=[],manifests={})
        root.joinpath("state.json").write_text(json.dumps(initial)); output=root/"outputs"; output.touch()
        env=dict(os.environ,PATH=f"{root}:{os.environ['PATH']}",GITHUB_SHA="a"*40,GH_REPO="example/KONTRA",GITHUB_RUN_ID="7",GITHUB_OUTPUT=str(output))
        def run(command): return subprocess.run(["bash","--noprofile","--norc","-e","-o","pipefail","-c",command],cwd=root,env=env,capture_output=True,text=True)
        result=run(publish); state=json.loads(root.joinpath("state.json").read_text())
        assert result.returncode==(1 if case in ("upload-fails","missing-asset","rotation-fails","bad-digest") else 0),(case,result.stderr)
        if case=="rotation-fails":
            assert state["published"] and len(state["releases"])==2 and any(r["tag_name"]=="nightly-previous" for r in state["releases"])
            result=run(publish); assert result.returncode==0,result.stderr
            state=json.loads(root.joinpath("state.json").read_text())
        promoted=case in ("current","first","cleanup-fails","rotation-fails")
        assert state["promoted"]==promoted,(case,state)
        assert not any(r["draft"] for r in state["releases"]),(case,state)
        if promoted:
            assert len(state["releases"])==(1 if case=="first" else 2)
            newest=next(r for r in state["releases"] if r["tag_name"]=="nightly")
            assert newest["target_commitish"]=="a"*40 and newest["name"]=="KONTRA "+version
            assert state["refs"]["v"+version]=="a"*40 and state["refs"]["nightly"]=="a"*40
            if case!="first": assert state["refs"]["nightly-previous"]=="b"*40
            assert len([tag for tag in state["refs"] if tag.startswith(("v","legacy-g"))])==(1 if case=="first" else 2)
            # A rerun must leave the same rollback and published release identities.
            before=state["releases"]
            result=run(publish); assert result.returncode==0,result.stderr
            state=json.loads(root.joinpath("state.json").read_text()); assert state["releases"]==before
            if case=="current":
                # The following snapshot must verify the previous versioned manifest.
                env["GITHUB_SHA"]="f"*40
                next_version="0.2.0-nightly.20261002.gffffffffffff"
                root.joinpath("Cargo.toml").write_text('[package]\nversion="'+next_version+'"\n')
                for platform in platforms:
                    path=root/f"dist/KONTRA-nightly-{platform}.zip"
                    with zipfile.ZipFile(path) as z: entries={n:z.read(n) for n in z.namelist()}
                    with zipfile.ZipFile(path,"w") as z:
                        for name,data in entries.items():
                            if name.endswith("build-info.json"):
                                info=json.loads(data);info.update(version=next_version,revision="f"*40);data=json.dumps(info).encode()
                            z.writestr(name,data)
                result=run(publish);assert result.returncode==0,result.stderr
                state=json.loads(root.joinpath("state.json").read_text())
                assert len(state["releases"])==2
                assert state["refs"]["nightly"]=="f"*40 and state["refs"]["nightly-previous"]=="a"*40
                assert "legacy-g"+"b"*12 not in state["refs"]
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
print("Nightly checks passed: 9 retention/rerun/upload/cleanup scenarios and four stable README links.")
