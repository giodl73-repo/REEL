"""Synthetic instrumental-mode boundaries; no private project material."""
import tempfile
from pathlib import Path
import unittest
from local_song_backend import validate_request, instrumental_report


class InstrumentalTests(unittest.TestCase):
    def request(self):
        return dict(schema='reel.local-song-request.v1', task='text2music', language='es',
                    duration_seconds=90, max_duration_seconds=90, seed=19,
                    instrumental=True, canonical_lyrics='', render_lyrics='[Instrumental]',
                    permissions=dict(private_only=True, external_upload=False,
                                     identifiable_imitation=False, public_release=False))

    def test_direct_instrumental_accepts_only_empty_sung_source(self):
        with tempfile.TemporaryDirectory() as folder:
            source=Path(folder)/'source.txt';source.write_bytes(b'')
            validate_request(self.request(), source)
            for update in [dict(instrumental='true'), dict(canonical_lyrics='invented'),
                           dict(render_lyrics='[Instrumental]\ninvented'),
                           dict(semantic_planner={'undeclared':True}),
                           dict(performance_repetitions=2), dict(duration_seconds=91)]:
                with self.subTest(update=update), self.assertRaises(ValueError):
                    validate_request(dict(self.request(), **update), source)
            source.write_text('accidental lyrics', encoding='utf-8')
            with self.assertRaises(ValueError):validate_request(self.request(), source)

    def test_empty_and_hallucinated_recognition_never_grant_approval(self):
        for passes,flag in [([dict(text='')],False),([dict(text='invented words')],True)]:
            report=instrumental_report(passes)
            self.assertEqual(report['recognized_text_present'],flag)
            self.assertIsNone(report['passed'])
            self.assertFalse(report['lyric_gate_applicable'])
            self.assertFalse(report['selected'])
            self.assertEqual(report['human_listening'],'pending')


if __name__ == '__main__':unittest.main()
