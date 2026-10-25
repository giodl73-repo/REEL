import unittest
import tempfile,json,sys,hashlib
from pathlib import Path
import numpy as np
from local_song_rhythm import summarize_envelope,harmonic_recurrence,validate_runtime,spectral_motion
class RhythmEvidence(unittest.TestCase):
 def pulses(self,intervals,rate=100,duration=80):
  x=np.zeros(rate*duration);times=np.cumsum(intervals)
  for t in times[times<duration]:x[int(t*rate)]=1
  return x
 def test_regular_and_jittered_pulses(self):
  rng=np.random.default_rng(731);steady=summarize_envelope(self.pulses(np.full(180,.5)),100);jitter=summarize_envelope(self.pulses(rng.uniform(.28,.72,180)),100)
  self.assertGreater(steady['short_cycle_repetition_median'],jitter['short_cycle_repetition_median']+.25)
  self.assertLess(steady['folded_local_tempo_iqr_percent'],jitter['folded_local_tempo_iqr_percent'])
 def test_syncopated_repeating_pattern_is_preserved(self):
  x=np.zeros(8000)
  for b in np.arange(0,80,4):
   for offset in [0,.75,1.5,2.5,3]:
    if b+offset<80:x[int((b+offset)*100)]=1
  result=summarize_envelope(x,100)
  self.assertGreater(result['long_cycle_repetition_median'],.95)
  self.assertLess(result['folded_local_tempo_iqr_percent'],1)
 def test_volume_invariance_and_silence(self):
  x=self.pulses(np.full(180,.5));a=summarize_envelope(x,100);b=summarize_envelope(x*.03,100)
  self.assertAlmostEqual(a['short_cycle_repetition_median'],b['short_cycle_repetition_median'])
  self.assertEqual(summarize_envelope(np.zeros(8000),100)['status'],'unmeasurable')
 def test_dense_steady_attacks_are_not_irregular(self):
  sparse=summarize_envelope(self.pulses(np.full(180,.5)),100);dense=summarize_envelope(self.pulses(np.full(700,.125)),100)
  self.assertGreater(dense['detected_attacks_per_second'],3*sparse['detected_attacks_per_second'])
  self.assertGreater(dense['short_cycle_repetition_median'],.9)
 def test_tempo_change_exceeds_fixed_pulse_drift(self):
  stationary=summarize_envelope(self.pulses(np.full(180,.5)),100);changing=summarize_envelope(self.pulses(60/np.linspace(100,140,180)),100)
  self.assertGreater(changing['folded_local_tempo_iqr_percent'],stationary['folded_local_tempo_iqr_percent']+3)
 def test_repeating_harmony_and_random_harmony(self):
  repeated=np.tile(np.eye(12)[:,[0,0,5,5,7,7,0,0]],(1,10));rng=np.random.default_rng(582);random=np.eye(12)[:,rng.integers(0,12,80)]
  a=harmonic_recurrence(repeated,1);b=harmonic_recurrence(random,1)
  self.assertGreater(a['harmonic_pattern_recurrence_median'],b['harmonic_pattern_recurrence_median']+.3)
 def test_short_harmonic_comparison_has_no_nan(self):
  self.assertEqual(harmonic_recurrence(np.ones((12,24)),1)['status'],'unmeasurable')
 def test_installed_dependency_mutation_fails_closed(self):
  with tempfile.TemporaryDirectory() as tmp:
   root=Path(tmp);dep=root/'dependency.py';dep.write_bytes(b'abc');exe=Path(sys.executable);runtime=dict(schema='reel.local-song-rhythm-runtime.v1',python=dict(sha256=hashlib.sha256(exe.read_bytes()).hexdigest(),bytes=exe.stat().st_size),files=[dict(path=str(dep),sha256=hashlib.sha256(b'abc').hexdigest(),bytes=3)]);config=root/'runtime.json';config.write_text(json.dumps(runtime),encoding='utf-8');validate_runtime(config);dep.write_bytes(b'abd')
   with self.assertRaisesRegex(ValueError,'Installed numeric runtime changed'):validate_runtime(config)
 def test_register_and_texture_motion_are_measured(self):
  f=np.linspace(0,10000,101);steady=np.zeros((101,80));steady[3]=1;changing=steady.copy()
  for t in range(1,80,2):changing[:,t]=0;changing[15,t]=1
  a=spectral_motion(steady,f,1);b=spectral_motion(changing,f,1)
  self.assertGreater(b['texture_change_mean'],a['texture_change_mean']+.5)
  self.assertGreater(b['harmonic_centroid_iqr_percent'],a['harmonic_centroid_iqr_percent']+20)
 def test_spectral_motion_is_gain_invariant(self):
  rng=np.random.default_rng(41);S=rng.uniform(0,1,(101,80));f=np.linspace(0,10000,101);a=spectral_motion(S,f,1);b=spectral_motion(S*20,f,1)
  self.assertAlmostEqual(a['texture_change_mean'],b['texture_change_mean'])
 def test_nonfinite_evidence_fails(self):
  with self.assertRaises(ValueError):summarize_envelope(np.full(8000,np.nan),100)
if __name__=='__main__':unittest.main()
