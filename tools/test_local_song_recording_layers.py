import copy
import unittest
from local_song_recording_layers import notation_voices, filter_layer, guitar_treatment,piano_bass_reduction


class RecordingLayerTests(unittest.TestCase):
    def test_declared_bass_reduction_preserves_selected_source_intervals(self):
        parts=[dict(notes=[dict(id=1,start=0,end=1,pitch=40),dict(id=2,start=0,end=.5,pitch=55),dict(id=3,start=1,end=2,pitch=43),dict(id=4,start=1,end=2,pitch=72)])];prior=copy.deepcopy(parts);out,ledger=piano_bass_reduction(parts,dict(source='piano',selection='lowest-attack',midi_low=28,midi_high=60,program=32),'trio');self.assertEqual(parts,prior);self.assertEqual(out[0]['notes'],[parts[0]['notes'][0],parts[0]['notes'][2]]);self.assertEqual(ledger['excluded_note_ids'],[2,4]);self.assertFalse(ledger['recovered_bass_stem'])
    def test_notation_polyphony_partition_preserves_sounding_events(self):
        notes=[dict(id='a',start_tick=0,duration_ticks=10,midi_note=60),dict(id='b',start_tick=0,duration_ticks=10,midi_note=64),dict(id='c',start_tick=10,duration_ticks=10,midi_note=67)];old=copy.deepcopy(notes);result=notation_voices(notes)
        self.assertEqual(notes,old);self.assertEqual([n['voice'] for n in result],[1,2,1]);self.assertEqual([{k:v for k,v in n.items() if k!='voice'} for n in result],notes)
    def test_excess_polyphony_is_not_dropped(self):
        with self.assertRaises(ValueError):notation_voices([dict(id=str(i),start_tick=0,duration_ticks=10,midi_note=40+i) for i in range(17)])
    def test_unsupported_source_parts_are_omitted(self):
        parts=[dict(id='p',program=0,drums=False,notes=[dict(start=1,end=2,pitch=60,velocity=100)])];result,count=filter_layer(parts,dict(programs=list(range(32,40)),drums=True,hand=False));self.assertEqual(result,[]);self.assertEqual(count,0)
    def test_hand_color_changes_no_source_clock(self):
        source=[dict(id='d',program=0,drums=True,notes=[dict(start=1,end=1.1,pitch=36,velocity=100),dict(start=2,end=2.1,pitch=38,velocity=100)])];old=copy.deepcopy(source);result,count=filter_layer(source,dict(programs=[],drums=True,hand=True));self.assertEqual(source,old);self.assertEqual(count,2);self.assertEqual([n['start'] for n in result[0]['notes']],[1,2]);self.assertEqual([n['pitch'] for n in result[0]['notes']],[60,61])
    def test_guitar_treatment_is_deterministic_and_original_is_immutable(self):
        notes=[dict(id=1,start=1.,end=2.,pitch=60,velocity=100),dict(id=2,start=1.,end=2.,pitch=64,velocity=100)];old=copy.deepcopy(notes);a,ledger=guitar_treatment(notes);b,_=guitar_treatment(notes);self.assertEqual(a,b);self.assertEqual(notes,old);self.assertEqual(len(ledger),2)


if __name__=='__main__':unittest.main()
