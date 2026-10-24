import unittest
from local_song_muscriptor import paired_notes, diagnose_events


class MeasuredNotesTests(unittest.TestCase):
    def events(self,start=1,end=2,pitch=60):
        return [dict(type='start',index=7,start_time=start,pitch=pitch,instrument='acoustic_piano'),dict(type='end',start_event_index=7,end_time=end)]
    def test_exact_measured_pair_is_preserved(self):
        notes,outside=paired_notes(self.events(),3);self.assertEqual(notes,[dict(id=7,start=1,end=2,pitch=60,instrument='acoustic_piano')]);self.assertEqual(outside,[])
    def test_missing_end_fails(self):
        with self.assertRaises(ValueError):paired_notes(self.events()[:1],3)
    def test_duplicate_end_fails(self):
        rows=self.events()
        with self.assertRaises(ValueError):paired_notes(rows+[rows[-1]],3)
    def test_reversed_duration_is_never_repaired(self):
        with self.assertRaises(ValueError):paired_notes(self.events(start=2,end=1),3)
    def test_zero_duration_is_never_filled(self):
        with self.assertRaises(ValueError):paired_notes(self.events(start=1,end=1),3)
    def test_invalid_pitch_fails(self):
        with self.assertRaises(ValueError):paired_notes(self.events(pitch=128),3)
    def test_generated_padding_is_quarantined_without_trimming(self):
        notes,outside=paired_notes(self.events(end=4),3);self.assertEqual(notes,[]);self.assertEqual(outside[0]['end'],4)
    def test_nan_timestamp_fails(self):
        with self.assertRaises(ValueError):paired_notes(self.events(start=float('nan')),3)
    def test_failed_raw_span_is_identified_without_repair(self):
        raw=self.events(start=1,end=1);issues=diagnose_events(raw);self.assertEqual(issues,[dict(id=7,reason='invalid-measured-span',start=1,end=1)]);self.assertEqual(raw[1]['end_time'],1)


if __name__=='__main__':unittest.main()
