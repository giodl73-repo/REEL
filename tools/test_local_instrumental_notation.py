import copy
import unittest
from local_instrumental_notation import reading_notes,nearest,chord_voices,grand_voice_groups


class ReadingTests(unittest.TestCase):
    def notes(self):return [dict(id='a',start_tick=170,duration_ticks=211,midi_note=60,voice=1),dict(id='b',start_tick=1000,duration_ticks=30,midi_note=67,voice=1)]
    def test_source_pitches_and_clock_are_reversible(self):
        notes=self.notes();old=copy.deepcopy(notes);out,ledger=reading_notes(notes,500000,612345,960,2000,70);self.assertEqual(notes,old)
        for n,p,r in zip(notes,out,ledger):
            self.assertEqual(n['midi_note'],p['midi_note']);self.assertEqual(p['start_tick']+r['onset_offset_ticks'],nearest(n['start_tick']*500000/612345));self.assertEqual(p['duration_ticks']+r['duration_offset_ticks'],nearest((n['start_tick']+n['duration_ticks'])*500000/612345)-nearest(n['start_tick']*500000/612345))
    def test_zero_source_duration_is_rejected(self):
        notes=self.notes();notes[0]['duration_ticks']=0
        with self.assertRaises(ValueError):reading_notes(notes,500000,500000,960,2000,0)
    def test_near_end_note_keeps_a_valid_offset_extent(self):
        notes=[dict(id='tail',start_tick=1995,duration_ticks=5,midi_note=60,voice=1)];out,ledger=reading_notes(notes,500000,500000,960,2000,70);self.assertLessEqual(out[0]['start_tick']+out[0]['duration_ticks'],2000);self.assertEqual(out[0]['duration_ticks']+ledger[0]['duration_offset_ticks'],5)
    def test_rounding_is_half_away_from_zero(self):
        self.assertEqual(nearest(.5),1);self.assertEqual(nearest(-.5),-1)
    def test_chords_share_voice_without_losing_source_notes(self):
        rows=[dict(id=str(i),start_tick=0,duration_ticks=480,midi_note=p,voice=i+1) for i,p in enumerate([60,64,67,72,76])];out=chord_voices(rows);self.assertEqual(len(out),5);self.assertEqual({n['voice'] for n in out},{1});self.assertEqual([n['midi_note'] for n in out],[60,64,67,72,76])
    def test_more_than_four_overlapping_independent_lines_is_rejected(self):
        with self.assertRaises(ValueError):chord_voices([dict(id=str(i),start_tick=i,duration_ticks=480,midi_note=60+i,voice=1) for i in range(5)])
    def test_dense_keyboard_uses_two_staves_without_losing_notes(self):
        rows=[dict(id=str(i),start_tick=i*120,duration_ticks=1440,midi_note=60+i,voice=1) for i in range(6)];upper,lower=grand_voice_groups(rows);self.assertEqual({n['id'] for n in upper+lower},{n['id'] for n in rows});self.assertTrue(all(1<=n['voice']<=4 for n in upper+lower));self.assertEqual(len(upper+lower),6)


if __name__=='__main__':unittest.main()
