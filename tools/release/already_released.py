"""Check whether any published GitHub release tag points at this checkout."""
import json
import os
from pathlib import Path


def already_released(tags, pages):
    return any(release['tag_name'] in tags and not release['draft']
               for page in pages for release in page)


def main():
    tags = set(Path('head-tags.txt').read_text().splitlines())
    pages = json.loads(Path('releases.json').read_text())
    with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
        print(f"released={str(already_released(tags, pages)).lower()}", file=output)


if __name__ == '__main__':
    main()
