import tomllib
import unittest

from locked_moose_revision import locked_revision


class LockedMooseRevisionTests(unittest.TestCase):
    def test_extracts_the_git_commit_with_or_without_a_ref_query(self):
        revision = "ab" * 20
        for query in ("", "?branch=main", f"?rev={revision}"):
            with self.subTest(query=query):
                lock = tomllib.loads(f'''[[package]]
name = "moose"
version = "7.0.0"
source = "git+https://github.com/Matari-Audio/moose{query}#{revision}"

[[package]]
name = "moose-au"
version = "7.0.0"
source = "git+https://github.com/Matari-Audio/moose#{revision}"
''')
                self.assertEqual(locked_revision(lock), revision)

    def test_rejects_path_unpinned_and_unexpected_sources(self):
        for source in (None, "git+https://github.com/Matari-Audio/moose",
                       "registry+https://github.com/rust-lang/crates.io-index",
                       f"git+https://github.com/Matari-Audio/moose-fork#{'ab' * 20}"):
            with self.subTest(source=source):
                package = {"name": "moose"}
                if source is not None:
                    package["source"] = source
                with self.assertRaisesRegex(ValueError, "no public Moose Git commit"):
                    locked_revision({"package": [package]})

    def test_rejects_an_ambiguous_lock(self):
        with self.assertRaisesRegex(ValueError, "exactly one"):
            locked_revision({"package": [{"name": "moose"}, {"name": "moose"}]})
