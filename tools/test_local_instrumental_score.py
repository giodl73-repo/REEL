import copy
import unittest
from local_instrumental_score import note_parts, verify_source_clock


class InstrumentalScoreTests(unittest.TestCase):
    def fixture(self):
        return dict(validation_passed=True,programs=dict(acoustic_piano=0,flutes=72),notes=[dict(id=1,start=.173,end=.991,pitch=60,instrument='acoustic_piano'),dict(id=2,start=.231,end=.719,pitch=72,instrument='flutes')])
    def test_polyphony_and_clock_survive_a_nonround_pulse(self):
        data=self.fixture();old=copy.deepcopy(data);parts,usq,total=note_parts(data,2,113.7,'flutes');proof=verify_source_clock(parts,data,usq);self.assertTrue(proof['every_selected_note_and_pitch_preserved']);self.assertEqual(data,old);self.assertEqual(sum(len(p['notes']) for p in parts),2)
    def test_missing_selected_lead_is_rejected(self):
        with self.assertRaises(ValueError):note_parts(self.fixture(),2,120,'violin')
    def test_no_automatic_lead_is_invented(self):
        parts,_,_=note_parts(self.fixture(),2,120);self.assertTrue(all(p['role']!='melody' for p in parts))
    def test_failed_raw_capture_is_rejected(self):
        data=self.fixture();data['validation_passed']=False
        with self.assertRaises(ValueError):note_parts(data,2,120)
    def test_missing_program_is_rejected(self):
        data=self.fixture();del data['programs']['flutes']
        with self.assertRaises(ValueError):note_parts(data,2,120)
    def test_bad_and_outside_spans_are_never_filled(self):
        for end in [.173,3]:
            data=self.fixture();data['notes'][0]['end']=end
            with self.assertRaises(ValueError):note_parts(data,2,120)
    def test_source_clock_detects_transposition_and_dropped_notes(self):
        data=self.fixture();parts,usq,_=note_parts(data,2,120);parts[0]['notes'][0]['midi_note']+=1
        with self.assertRaises(ValueError):verify_source_clock(parts,data,usq)
        parts,usq,_=note_parts(data,2,120);parts[0]['notes']=[]
        with self.assertRaises(ValueError):verify_source_clock(parts,data,usq)
    def test_source_clock_detects_retiming(self):
        data=self.fixture();parts,usq,_=note_parts(data,2,120);parts[0]['notes'][0]['start_tick']+=100
        with self.assertRaises(ValueError):verify_source_clock(parts,data,usq)
    def test_duplicate_identity_is_rejected(self):
        data=self.fixture();data['notes'][1]['id']=1
        with self.assertRaises(ValueError):note_parts(data,2,120)
    def test_explicit_part_projection_preserves_every_selected_source_event(self):
        data=self.fixture();old=copy.deepcopy(data);parts,usq,_=note_parts(data,2,120,'flutes',['acoustic_piano']);proof=verify_source_clock(parts,data,usq,excluded_instruments=['acoustic_piano']);self.assertEqual(data,old);self.assertEqual(proof['notes'],1);self.assertEqual(proof['omitted_source_events'],1);self.assertEqual(parts[0]['notes'][0]['source_id'],2)
    def test_unknown_exclusion_and_empty_projection_are_rejected(self):
        with self.assertRaises(ValueError):note_parts(self.fixture(),2,120,None,['drums'])
        with self.assertRaises(ValueError):note_parts(self.fixture(),2,120,None,['flutes','acoustic_piano'])


if __name__=='__main__':unittest.main()
