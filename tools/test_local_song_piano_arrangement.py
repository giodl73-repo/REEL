import unittest
from local_song_piano_arrangement import chord_path,triads,voicing,audit_notes,midi_events


class PianoTests(unittest.TestCase):
    def test_renderer_tempo_on_part_track_is_honored(self):
        import mido,tempfile
        from pathlib import Path
        midi=mido.MidiFile(ticks_per_beat=480)
        midi.tracks.append(mido.MidiTrack([mido.MetaMessage('end_of_track',time=0)]))
        midi.tracks.append(mido.MidiTrack([mido.MetaMessage('set_tempo',tempo=1000000,time=0),mido.Message('note_on',note=60,velocity=64,time=480),mido.Message('note_off',note=60,velocity=0,time=960)]))
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'input.mid';midi.save(path)
            self.assertEqual(midi_events(path),[dict(pitch=60,start=1.,end=3.)])

    def test_clear_minor_and_major_evidence(self):
        import numpy as np
        values=np.zeros((6,12));values[:3,[0,4,7]]=1;values[3:,[9,0,4]]=1
        path,_=chord_path(values)
        self.assertEqual([triads()[i][:2] for i in path],[(0,'major')]*3+[(9,'minor')]*3)

    def test_close_voicing_stays_playable_and_keeps_pitch_classes(self):
        previous=None
        for root,_,pcs in triads():
            result=voicing(root,pcs,previous)
            self.assertEqual(sorted(n%12 for n in result),sorted(pcs))
            self.assertLessEqual(max(result)-min(result),12)
            self.assertTrue(all(48<=n<=72 for n in result));previous=result

    def test_drop_is_rejected(self):
        with self.assertRaises(ValueError):audit_notes([dict(pitch=60,start=1,end=2)],[])

    def test_source_clock_drift_is_rejected(self):
        with self.assertRaises(ValueError):audit_notes([dict(pitch=60,start=1,end=2)],[dict(pitch=60,start=1.2,end=2.2)])

    def test_small_tick_roundoff_is_allowed(self):
        result=audit_notes([dict(pitch=60,start=1,end=2)],[dict(pitch=60,start=1.001,end=2.001)])
        self.assertTrue(result['passed'])


if __name__=='__main__':unittest.main()
