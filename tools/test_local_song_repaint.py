import unittest
import numpy as np
from local_song_repaint import splice, validate


class RepaintTests(unittest.TestCase):
    def test_only_mask_changes(self):
        source=np.arange(200,dtype=np.float32).reshape(100,2)/200
        result=splice(source,np.zeros_like(source),10,2,5,.2)
        np.testing.assert_array_equal(result[:20],source[:20])
        np.testing.assert_array_equal(result[50:],source[50:])
        self.assertTrue(np.all(result[22:48]==0))

    def test_insertion_must_be_masked(self):
        p=dict(schema='reel.local-song-repaint.v1',patches=[dict(start=1,end=5,lyrics='invented words')],insertion=dict(at=7,seconds=2))
        with self.assertRaises(ValueError):validate(p,100,10)

    def test_overlap_rejected(self):
        p=dict(schema='reel.local-song-repaint.v1',patches=[dict(start=1,end=5,lyrics='invented'),dict(start=4,end=6,lyrics='words')])
        with self.assertRaises(ValueError):validate(p,100,10)


if __name__=='__main__':unittest.main()
