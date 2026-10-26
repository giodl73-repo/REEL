import unittest
import numpy as np
from local_song_lyric_transcript import activity_window,shifted_timestamp


class TranscriptClockTests(unittest.TestCase):
 def test_default_preserves_complete_recording(self):
  self.assertEqual(activity_window(np.r_[np.zeros(4000),np.ones(1000)],1000),(0,5000))
 def test_crop_retains_padding_and_original_clock(self):
  signal=np.r_[np.zeros(4000),np.ones(2000),np.zeros(4000)]
  start,end=activity_window(signal,1000,True)
  self.assertEqual((start,end),(2500,7500))
  self.assertEqual(shifted_timestamp([.2,.6],start/1000),[2.7,3.1])
 def test_silence_does_not_invent_an_active_window(self):
  self.assertEqual(activity_window(np.zeros(3000),1000,True),(0,3000))
 def test_unknown_word_endpoint_stays_unknown(self):
  self.assertEqual(shifted_timestamp([None,1.2],2),[None,3.2])
 def test_truthy_string_is_not_an_explicit_trim_choice(self):
  with self.assertRaises(ValueError):activity_window(np.ones(1000),1000,'true')


if __name__=='__main__':unittest.main()
