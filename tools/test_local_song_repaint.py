import unittest
import numpy as np
from local_song_repaint import splice, validate, validate_source_mode


class RepaintTests(unittest.TestCase):
    def test_source_preservation_controls(self):
        plan=dict(schema='reel.local-song-repaint.v1',patches=[dict(start=1,end=5,lyrics='[Instrumental]')])
        for mode in ['conservative','balanced','aggressive']:
            validate(dict(plan,repaint_mode=mode,repaint_strength=0),100,10)
            validate(dict(plan,repaint_mode=mode,repaint_strength=1),100,10)
        for strength in [True,-.1,1.1,float('nan'),'0.5']:
            with self.assertRaises(ValueError):validate(dict(plan,repaint_strength=strength),100,10)
        with self.assertRaises(ValueError):validate(dict(plan,repaint_mode='unknown'),100,10)

    def test_instrumental_patch_and_source_scope(self):
        permissions=dict(private_only=True,external_upload=False,identifiable_imitation=False,public_release=False)
        plan=dict(schema='reel.local-song-repaint.v1',instrumental=True,permissions=permissions,patches=[dict(start=2,end=7,lyrics='[Instrumental]')])
        request=dict(instrumental=True,canonical_lyrics='',render_lyrics='[Instrumental]',permissions=permissions)
        validate(plan,100,10);validate_source_mode(plan,request)
        with self.assertRaises(ValueError):validate_source_mode(plan,dict(request,instrumental=False))
        with self.assertRaises(ValueError):validate_source_mode(plan,dict(request,canonical_lyrics='invented'))
        with self.assertRaises(ValueError):validate_source_mode(dict(plan,permissions={}),request)
        with self.assertRaises(ValueError):validate(dict(plan,patches=[dict(start=2,end=7,lyrics='invented words')]),100,10)
        with self.assertRaises(ValueError):validate(dict(plan,instrumental='true'),100,10)

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
