import copy
import unittest
from local_song_note_quarantine import project


class QuarantineTests(unittest.TestCase):
    def raw(self,n=300):
        return [e for i in range(n) for e in [dict(type='start',index=i,start_time=i/100,pitch=60,instrument='piano'),dict(type='end',start_event_index=i,end_time=i/100+.005)]]
    def test_bounded_zero_span_is_excluded_without_changing_others(self):
        rows=self.raw();rows[1]['end_time']=0;old=copy.deepcopy(rows);notes,rejected,p=project(rows,4,dict(piano=0));self.assertEqual(rows,old);self.assertEqual(len(notes),299);self.assertEqual(rejected[0]['id'],0);self.assertEqual(notes[0]['end'],.015);self.assertEqual(p['rejected_notes'],1)
    def test_mass_failure_is_not_relaxed(self):
        rows=self.raw();rows[1]['end_time']=0;rows[3]['end_time']=.01
        with self.assertRaises(ValueError):project(rows,4,dict(piano=0))
    def test_unknown_instrument_fails_instead_of_being_dropped(self):
        with self.assertRaises(ValueError):project(self.raw(),4,{})
    def test_unpaired_identity_is_explicitly_quarantined(self):
        rows=self.raw()[:-1];notes,rejected,p=project(rows,4,dict(piano=0));self.assertEqual(rejected[0]['reason'],'ambiguous-or-missing-event-pair');self.assertEqual(len(notes),299)
    def test_invalid_pitch_is_never_transposed(self):
        rows=self.raw();rows[0]['pitch']=128;notes,rejected,p=project(rows,4,dict(piano=0));self.assertEqual(rejected[0]['start_events'][0]['pitch'],128);self.assertTrue(all(n['pitch']==60 for n in notes))
    def test_no_blanket_allowance(self):
        with self.assertRaises(ValueError):project(self.raw(),4,dict(piano=0),maximum_fraction=.1)
    def test_outside_span_is_never_trimmed(self):
        rows=self.raw();rows[-1]['end_time']=5;notes,rejected,p=project(rows,4,dict(piano=0));self.assertEqual(rejected[0]['end_events'][0]['end_time'],5);self.assertEqual(len(notes),299)


if __name__=='__main__':unittest.main()
