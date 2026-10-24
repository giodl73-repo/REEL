import tempfile
from pathlib import Path
import unittest
from fractions import Fraction
from local_instrumental_engrave import intervals,prepare_musicxml,duration_pieces,text_bounds


class IntervalTests(unittest.TestCase):
    def test_music_metric_overhang_does_not_engulf_in_bounds_chord(self):
        blocks=[dict(lines=[dict(spans=[dict(text='\ue000',bbox=(20,-19,25,360)),dict(text='Em',bbox=(30,20,45,35))])])]
        outside,metrics=text_bounds(blocks,612,792);self.assertEqual(outside,[]);self.assertEqual(len(metrics),1)
        blocks[0]['lines'][0]['spans'][1]['bbox']=(30,-5,45,35)
        self.assertEqual(len(text_bounds(blocks,612,792)[0]),1)

    def parse(self,body):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'score.xml';p.write_text('<score-partwise><part-list/><part id="P1">'+body+'</part></score-partwise>');return intervals(p)

    def test_chords_ties_backup_and_divisions(self):
        x=self.parse('<measure><attributes><divisions>2</divisions></attributes><note><pitch><step>C</step><octave>4</octave></pitch><duration>2</duration><tie type="start"/></note><note><chord/><pitch><step>E</step><octave>4</octave></pitch><duration>2</duration></note><backup><duration>2</duration></backup><note><pitch><step>G</step><octave>3</octave></pitch><duration>2</duration><voice>2</voice></note></measure><measure><note><pitch><step>C</step><octave>4</octave></pitch><duration>2</duration><tie type="stop"/></note></measure>')
        self.assertEqual(x[(0,60,Fraction(0),Fraction(2))],1);self.assertEqual(x[(0,64,Fraction(0),Fraction(1))],1);self.assertEqual(sum(x.values()),3)

    def test_broken_ties_rejected(self):
        with self.assertRaises(ValueError):self.parse('<measure><note><pitch><step>C</step><octave>4</octave></pitch><duration>1</duration><tie type="stop"/></note></measure>')

    def test_explicit_spelling_retains_chords_and_two_staff_clocks(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'source.xml';target=Path(d)/'typed.xml'
            p.write_text('<score-partwise><part-list/><part id="P1"><measure><attributes><divisions>960</divisions></attributes><note><pitch><step>C</step><octave>4</octave></pitch><duration>600</duration><voice>1</voice><staff>1</staff></note><note><chord/><pitch><step>E</step><octave>4</octave></pitch><duration>600</duration><voice>1</voice><staff>1</staff></note><backup><duration>600</duration></backup><note><pitch><step>C</step><octave>3</octave></pitch><duration>600</duration><voice>1</voice><staff>2</staff></note></measure></part></score-partwise>')
            prepare_musicxml(p,target);self.assertEqual(intervals(p),intervals(target));self.assertIn('<voice>5</voice>',target.read_text());self.assertIn('<tie type="start"',target.read_text())

    def test_irregular_duration_spelling_is_exact(self):
        for ticks in [1,31,120,360,600,1080,3839]:
            parts=duration_pieces(ticks,960);self.assertEqual(sum(v[0] for v in parts),ticks)


if __name__=='__main__':unittest.main()
