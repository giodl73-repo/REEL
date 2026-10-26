import tempfile,unittest
from pathlib import Path
from local_songbook_exact_midi import export

class ExactMidiTests(unittest.TestCase):
    def test_fifths_and_ties_keep_exact_quarter_clock(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'score.musicxml';p.write_text('<score-partwise><part-list/><part id="P1"><measure><attributes><divisions>10080</divisions></attributes><note><pitch><step>C</step><octave>4</octave></pitch><duration>4032</duration><tie type="start"/></note></measure><measure><note><pitch><step>C</step><octave>4</octave></pitch><duration>1008</duration><tie type="stop"/></note></measure></part></score-partwise>')
            q=export(p,Path(d)/'editable.mid');self.assertTrue(q['pitch_counts_equal']);self.assertEqual(q['maximum_onset_error_quarters'],0);self.assertEqual(q['maximum_duration_error_quarters'],0);self.assertFalse(q['source_score_modified'])

if __name__=='__main__':unittest.main()
