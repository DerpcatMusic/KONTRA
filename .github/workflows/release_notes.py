"""Reviewed changelog deltas plus complete shipped public history, without local Git history."""
import base64
from pathlib import Path
import re
import subprocess

SECTIONS = ("Added", "Changed", "Fixed", "Known limits")


def entry(text, version):
    for part in re.split(r"(?m)^## ", text)[1:]:
        title, _, body = part.partition("\n")
        if title.split(" — ", 1)[0] == version:
            return "## " + title + "\n" + body.strip()
    return ""


def sections(text):
    result = {name: [] for name in SECTIONS}
    unreleased = next((p for p in re.split(r"(?m)^## ", text)[1:]
                       if "unreleased" in p.partition("\n")[0].lower()), "")
    for part in re.split(r"(?m)^### ", unreleased)[1:]:
        name, _, body = part.partition("\n")
        name = "Known limits" if name.strip() == "Compatibility" else name.strip()
        if name in result:
            result[name].extend(b.strip() for b in re.split(r"\n\s*\n|(?=^- )", body, flags=re.M) if b.strip())
    return result


def normalized(block):
    return " ".join(block.split())


def render(repo, revision, version, checkpoint, current, previous_text, previous, commits, prs):
    """Pure rendering makes source/checkpoint selection and delta coverage testable."""
    frozen = entry(current, version)
    if frozen:
        assert revision in frozen and checkpoint in frozen, "Frozen entry has a different source checkpoint"
        body = frozen
    else:
        now, before = sections(current), sections(previous_text)
        assert any(now.values()), "CHANGELOG.md must contain reviewed release notes"
        lines = [f"## Changelog — {version}", "", f"Reviewed export checkpoint: `{checkpoint}`.", ""]
        for name in SECTIONS:
            old = {normalized(b) for b in before[name]}
            changes = now[name] if name == "Known limits" else [b for b in now[name] if normalized(b) not in old]
            lines += [f"### {name}", "", "\n\n".join(changes) or "- No reviewed changes in this category since the previous release.", ""]
        body = "\n".join(lines).strip()
    if previous:
        base = previous["target_commitish"]
        body += f"\n\n[Complete public comparison](https://github.com/{repo}/compare/{base}...{revision})."
        if not previous_text:
            body += "\nThe previous source has no changelog; this entry includes the complete reviewed source notes."
    else:
        body += "\n\nFirst published snapshot: the complete reviewed source notes are included."
    body += "\n\n### Complete shipped public commit messages\n"
    assert commits, "Release notes must include the actual public source commit"
    for commit in commits:
        body += f"\n[{commit['sha'][:12]}](https://github.com/{repo}/commit/{commit['sha']})\n\n"
        body += "\n".join("> " + line for line in commit["commit"]["message"].splitlines()) + "\n"
    if prs:
        body += "\n### Complete merged PR descriptions\n"
        for pr in prs:
            body += f"\n#### [{pr['title']}]({pr['html_url']})\n\n{(pr.get('body') or '').strip() or 'No description was supplied.'}\n"
    return body.strip() + "\n"


def generate(api, repo, revision, version, previous, changelog=None):
    current = Path("CHANGELOG.md").read_text() if changelog is None else changelog
    source = Path("SOURCE_COMMIT")
    checkpoint = source.read_text().strip() if source.exists() else revision
    previous_text = ""
    if previous:
        base = previous["target_commitish"]
        try:
            content = api("contents/CHANGELOG.md?ref=" + base)
            previous_text = base64.b64decode(content["content"]).decode()
        except subprocess.CalledProcessError as error:
            if b"HTTP 404" not in (error.stderr or b""):
                raise
        pages = api(f"compare/{base}...{revision}?per_page=100", "--paginate", "--slurp")
        assert pages[0]["status"] in ("ahead", "identical"), "Previous release must precede this source"
        commits = [c for page in pages for c in page["commits"]]
        assert len(commits) == pages[0]["total_commits"], "Incomplete public commit pagination"
        if not commits:
            commits = [api("commits/" + revision)]
    else:
        commits = [api("commits/" + revision)]
    shipped = {c["sha"] for c in commits}
    prs = {}
    for commit in commits:
        pages = api(f"commits/{commit['sha']}/pulls?per_page=100", "--paginate", "--slurp")
        for page in pages:
            for pr in page:
                if pr.get("merged_at") and pr.get("merge_commit_sha") in shipped:
                    prs[pr["number"]] = pr
    return render(repo, revision, version, checkpoint, current, previous_text, previous,
                  commits, sorted(prs.values(), key=lambda p: (p["merged_at"], p["number"])))
