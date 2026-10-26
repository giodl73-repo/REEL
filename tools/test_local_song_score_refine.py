import unittest
import numpy as np
import copy
from unittest.mock import patch, MagicMock
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
 def test_zero_additions_preserve_notes_without_invalid_empty_analysis(self):
  import local_song_score_refine as module
  note=dict(id='old',start_tick=0,duration_ticks=10,midi_note=69,voice=1)
  parent=dict(model_id='fixture',source={},tempo_map=[dict(microseconds_per_quarter=1000000)],musical_timebase=dict(pulses_per_quarter=100),parts=[dict(notes=[note])],lead_sheet=dict(underlay=[]),analyses=[],unknowns=[])
  evidence=dict(syllables=[dict(note_ids=[],line=1,mean_log_probability=-5,span_seconds=[0,.1])],owner_accepted_omissions=[],request=dict(recording={},vocal_estimate={}))
  q=dict(schema='reel.song-score-refinement.v1',permissions=dict(private_only=True,external_upload=False,public_release=False),runtime_packages={},model={},evidence={},frames={},pitch_evidence={},expected_singing={},native_runtime={})
  data={'refine.json':q,'parent-model.json':parent,'parent-evidence.json':evidence,'pitch-evidence.json':dict(source={},vocal_estimate={}),'source.json':dict(media=dict(timebase=dict(sample_rate_hz=100)))}
  framefile=MagicMock();framefile.__enter__.return_value=dict(hop=1,sample_rate=100)
  written={}
  with patch.object(module,'offline'),patch.object(module,'checked'),patch.object(module,'read',side_effect=lambda name:copy.deepcopy(data[name])),patch.object(module,'write',side_effect=lambda name,value:written.update({name:copy.deepcopy(value)})),patch.object(module,'native',return_value={}) as native,patch('local_song_backend.descriptor',return_value={'sha256':'0'*64}),patch('numpy.load',return_value=framefile),patch.object(module.os,'link'):
   module.main()
  self.assertFalse(any(call.args[0]=='music-analysis-validate' for call in native.call_args_list))
  self.assertEqual(written['model.json']['parts'][0]['notes'],[note])
  self.assertEqual(written['model.json']['analyses'],[])
  self.assertEqual(written['refinement-analysis.json']['added_observations'],0)
if __name__=='__main__':unittest.main()
