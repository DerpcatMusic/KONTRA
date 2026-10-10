"""Check the static site, including GitHub Pages project-relative links."""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit, unquote

root = Path(__file__).resolve().parent

class Page(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.path, self.links, self.ids, self.headings = path, [], set(), 0
    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            assert attrs['id'] not in self.ids, f'duplicate ID in {self.path}'
            self.ids.add(attrs['id'])
        if tag == 'h1': self.headings += 1
        if tag == 'img': assert attrs.get('alt'), f'missing alt in {self.path}'
        for key in ('href', 'src'):
            if key in attrs: self.links.append(attrs[key])

pages = {}
for path in root.glob('*.html'):
    page = Page(path)
    page.feed(path.read_text())
    assert page.headings == 1, f'expected one h1: {path}'
    assert 'main' in page.ids
    pages[path] = page
assert len(pages) == 4
for path, page in pages.items():
    for link in page.links:
        parsed = urlsplit(link)
        if parsed.scheme:
            assert parsed.scheme == 'https', link
            continue
        assert not parsed.path.startswith('/'), f'project-relative URL required: {link}'
        target = (path.parent / unquote(parsed.path)).resolve() if parsed.path else path
        assert target.is_relative_to(root) and target.is_file(), f'broken link: {path.name}: {link}'
        if parsed.fragment:
            assert target in pages and unquote(parsed.fragment) in pages[target].ids, f'broken anchor: {link}'
assert (root / 'assets/FONT-LICENSE.txt').is_file()
assert (root / 'assets/PROVENANCE.md').is_file()
print(f'Checked {len(pages)} pages: assets, internal links, anchors, image text and project-relative paths.')
