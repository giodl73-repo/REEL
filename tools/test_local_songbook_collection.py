import tempfile
import unittest
from pathlib import Path
from local_songbook_collection import paired_lines, scoped_path


class CollectionTests(unittest.TestCase):
    def test_independent_line_counts_preserve_both_literals(self):
        pairs=[dict(es='árbol\ncaído',en='fallen\ntree\nat rest'),dict(es='fin',en='end')]
        rows=paired_lines(pairs)
        self.assertEqual([a for a,b in rows if a],['árbol','caído','fin'])
        self.assertEqual([b for a,b in rows if b],['fallen','tree','at rest','end'])

    def test_output_escape_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory).resolve()
            self.assertEqual(scoped_path(root,'scores/one.pdf'),root/'scores/one.pdf')
            with self.assertRaises(ValueError):scoped_path(root,'../foreign.pdf')
            with self.assertRaises(ValueError):scoped_path(root,'.')


if __name__=='__main__':
    unittest.main()
