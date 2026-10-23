import unittest
import numpy as np
from local_song_score_refine import supported_runs

class FrameRefineTests(unittest.TestCase):
 def run_frames(self,f0=None,valid=None,mix=None,prob=None,start=0,end=.1):
  f0=np.full(10,440.) if f0 is None else f0
  return supported_runs(f0,np.ones(10,dtype=bool) if valid is None else valid,np.full(10,440.) if mix is None else mix,np.full(10,.9) if prob is None else prob,.01,start,end)
 def test_stable_supported_run_and_bounds(self):
  result=self.run_frames(start=.02,end=.09);self.assertEqual(len(result),1);self.assertEqual(result[0]['midi_note'],69);self.assertAlmostEqual(result[0]['start'],.02);self.assertAlmostEqual(result[0]['end'],.09)
 def test_octave_bleed_is_rejected(self):self.assertEqual(self.run_frames(mix=np.full(10,220.)),[])
 def test_low_probability_is_not_relaxed(self):self.assertEqual(self.run_frames(prob=np.full(10,.64)),[])
 def test_invalid_frames_split_support(self):self.assertEqual(self.run_frames(valid=np.array([1,1,1,0,1,1,1,0,1,1],dtype=bool)),[])
 def test_different_pitches_do_not_accumulate(self):
  f=np.array([440,440,440,466.164,440,440,440,466.164,440,440.]);self.assertEqual(self.run_frames(f0=f,mix=f),[])
 def test_short_character_span_remains_unmapped(self):self.assertEqual(self.run_frames(start=.01,end=.03),[])
if __name__=='__main__':unittest.main()
