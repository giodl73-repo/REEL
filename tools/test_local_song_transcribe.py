import unittest
import numpy as np
from local_song_transcribe import note_runs


class NoteRunsTests(unittest.TestCase):
    def test_vibrato_keeps_stable_note_and_unknown_gap(self):
        midi=np.r_[60+np.sin(np.arange(40))*.2,np.full(10,np.nan),np.full(40,62.)]
        valid=np.isfinite(midi)
        result=note_runs(midi,valid,np.ones(90)*.9,.01)
        self.assertEqual([e['midi_note'] for e in result],[60,62])
        self.assertLessEqual(result[0]['end'],.4)
        self.assertGreaterEqual(result[1]['start'],.5)

    def test_short_spurious_note_is_not_invented_as_sustain(self):
        result=note_runs(np.r_[np.full(5,70.),np.full(40,64.)],np.ones(45,dtype=bool),np.ones(45),.01)
        self.assertEqual([e['midi_note'] for e in result],[64])

    def test_no_voiced_frames_produces_no_notes(self):
        self.assertEqual(note_runs(np.full(50,np.nan),np.zeros(50,dtype=bool),np.zeros(50),.01),[])


if __name__=='__main__':unittest.main()
