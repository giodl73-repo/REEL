import copy
import unittest

from local_song_line_alignment import align
from local_song_word_underlay import propose, render_review


class UnderlayTests(unittest.TestCase):
    def fixture(self, text='Mi corazón'):
        model = dict(tempo_map=[dict(tick=0, microseconds_per_quarter=1000000)],
                     musical_timebase=dict(pulses_per_quarter=100), duration_ticks=1000,
                     parts=[dict(notes=[dict(id='n1', start_tick=100, duration_ticks=80),
                                        dict(id='n2', start_tick=200, duration_ticks=80)])])
        analysis = dict(passes=[dict(name='independent', tokens=[
            dict(word='mi', span=[1, 1.8]), dict(word='corazon', span=[2, 2.8])])])
        return model, analysis, align(text, text, analysis, model)

    def test_utf8_word_ranges_and_unmodified_model(self):
        text = 'Mi corazón'
        model, analysis, alignment = self.fixture(text)
        original = copy.deepcopy(model)
        result = propose(text, alignment, analysis, model)
        items = result['phrases'][0]['words']
        self.assertEqual(items[1]['note_ids'], ['n2'])
        self.assertEqual(text.encode()[items[1]['text_start_byte']:items[1]['text_end_byte']].decode(), 'corazón')
        self.assertEqual(model, original)
        self.assertFalse(result['native_underlay_export_ready'])

    def test_shared_note_requires_articulation_review(self):
        model, analysis, _ = self.fixture()
        model['parts'][0]['notes'] = [dict(id='shared', start_tick=100, duration_ticks=180)]
        alignment = align('Mi corazón', 'Mi corazón', analysis, model)
        report = propose('Mi corazón', alignment, analysis, model)
        self.assertTrue(all('articulation-review' in w['status'] for w in report['phrases'][0]['words']))
        self.assertEqual(report['word_proposals'], 0)

    def test_owner_omission_cannot_receive_underlay_or_listening_binding(self):
        model, analysis, _ = self.fixture()
        alignment = align('Mi corazón', 'Mi corazón', analysis, model, omitted=[1])
        self.assertEqual(propose('Mi corazón', alignment, analysis, model)['phrases'], [])
        with self.assertRaises(ValueError):
            propose('Mi corazón', alignment, analysis, model, [dict(line=1, span_seconds=[1, 3])])

    def test_missing_word_remains_unmapped(self):
        text = 'Mi gran corazón'
        model, analysis, alignment = self.fixture(text)
        report = propose(text, alignment, analysis, model)
        middle = report['phrases'][0]['words'][1]
        self.assertEqual(middle['word'], 'gran')
        self.assertEqual(middle['note_ids'], [])
        self.assertEqual(middle['status'], 'word-clock-unresolved')

    def test_listening_window_is_not_word_presence(self):
        model, analysis, alignment = self.fixture()
        report = propose('Mi corazón', alignment, analysis, model,
                         [dict(line=1, span_seconds=[1, 3], rationale='context only')])
        window = report['listening_windows'][0]
        self.assertFalse(window['underlay_assigned'])
        self.assertIn('not lexical confirmation', window['status'])

    def test_reused_recognizer_token_does_not_become_two_sung_lines(self):
        text = 'Mi corazón\nMi corazón'
        model, analysis, alignment = self.fixture(text)
        report = propose(text, alignment, analysis, model)
        self.assertTrue(all('token-reused' in w['status'] for p in report['phrases'] for w in p['words']))
        self.assertEqual(report['word_proposals'], 0)

    def test_browser_review_escapes_titles_and_uses_full_source(self):
        model, analysis, alignment = self.fixture()
        report = propose('Mi corazón', alignment, analysis, model)
        page = render_review([dict(title='<script>bad</script>', report=report,
                                   audio_uri='file:///E:/full.wav', report_uri='file:///E:/proposal.json')])
        self.assertIn('&lt;script&gt;bad&lt;/script&gt;', page)
        self.assertNotIn('<script>bad</script>', page)
        self.assertIn('src="file:///E:/full.wav"', page)
        self.assertIn('data-start="0.0000"', page)
        self.assertIn('not finished syllable engraving', page)


if __name__ == '__main__':
    unittest.main()
