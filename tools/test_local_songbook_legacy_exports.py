import unittest
from collections import Counter
from fractions import Fraction as F
from pathlib import Path
import tempfile
from local_songbook_legacy_exports import compare, midi_notes


class LegacyExportTests(unittest.TestCase):
    def test_gate_is_reported_without_claiming_nominal_duration(self):
        report = compare(Counter({(0, 60, F(0), F(1)): 1}),
                         [(60, F(0), F(9, 10))], 960)
        self.assertTrue(report['nominal_onsets_within_two_ticks'])
        self.assertAlmostEqual(report['maximum_duration_error_quarters'], .1)
        self.assertFalse(report['human_musical_approval'])

    def test_missing_duplicate_unison_is_rejected(self):
        with self.assertRaises(ValueError):
            compare(Counter({(0, 60, F(0), F(1)): 2}), [(60, F(0), F(1))], 480)

    def test_pitch_change_is_rejected(self):
        with self.assertRaises(ValueError):
            compare(Counter({(0, 60, F(0), F(1)): 1}), [(61, F(0), F(1))], 480)

    def test_coincident_unisons_and_independent_channels_survive(self):
        import mido
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'fixture.mid'
            midi = mido.MidiFile(ticks_per_beat=960)
            track = mido.MidiTrack(); midi.tracks.append(track)
            track.extend([mido.Message('note_on', note=60, channel=0, time=0),
                          mido.Message('note_on', note=60, channel=1, time=0),
                          mido.Message('note_off', note=60, channel=0, time=960),
                          mido.Message('note_off', note=60, channel=1, time=0)])
            midi.save(path)
            notes, ppq = midi_notes(path)
            report = compare(Counter({(0, 60, F(0), F(1)): 2}), notes, ppq)
            self.assertEqual(report['midi_notes'], 2)


if __name__ == '__main__':
    unittest.main()
