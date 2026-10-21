"""Synthetic opaque-token contract fixtures; no private lyrics or model/audio."""
import tempfile
from pathlib import Path
import unittest
import local_song_backend as backend
import local_song_campaign  # Campaign syntax/import must remain dependency-free.


def evidence(text):
    return [dict(name='synthetic',tokens=[dict(word=w,span=[i,i+1]) for i,w in enumerate(backend.words(text))])]


class LyricGateTests(unittest.TestCase):
    def test_phrase_wrapping_preserves_words_and_sentence_boundaries(self):
        source='Amber lamps\nshine above us.\nSilver bells\nring softly\n\nCopper wheels turn\n'
        rendered=backend.format_lyrics(source,True,12)
        self.assertIn('Amber lamps shine above us.\nSilver bells ring softly',rendered)
        self.assertIn('[Verse]\nCopper wheels turn',rendered)
        repeated=backend.format_repeated_sections(source,2,12)
        self.assertEqual(repeated.count('Amber lamps shine above us.'),2)
        self.assertEqual(repeated.count('Silver bells ring softly'),2)

    def test_repeated_sections_validate_exact_words_and_reject_rewrites(self):
        import re
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as folder:
            source=Path(folder)/'source.txt';source.write_text('Amber lamps\nshine\nSilver bells\nring\nCopper wheels turn\n',encoding='utf-8')
            campaign=local_song_campaign.Campaign(folder,Path(folder)/'unused.exe',Path(folder)/'work')
            campaign.repeat_section_lines=4;campaign.repeat_after_failures=1
            slot=dict(id='synthetic-a',title='Synthetic',source=backend.descriptor(source),source_lock={},
                duration_seconds=120,caption='An invented test brief',seed_base=100)
            with patch.object(campaign,'locate',return_value=source):request=campaign.request(slot,6)
            backend.validate_request(request,source)
            self.assertEqual(request['duration_seconds'],210)
            body=re.sub(r'^\s*\[[^\]\n]+\]\s*$','',request['render_lyrics'],flags=re.M)
            self.assertEqual(backend.words(body),backend.words('Amber lamps shine Silver bells ring')*2+backend.words('Copper wheels turn')*2)
            request['render_lyrics']=request['render_lyrics'].replace('Amber','Changed',1)
            with self.assertRaises(ValueError):backend.validate_request(request,source)

    def test_explicit_planner_preserves_single_source_pass(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as folder:
            source=Path(folder)/'source.txt';source.write_text('Amber lamps\nshine\n',encoding='utf-8')
            campaign=local_song_campaign.Campaign(folder,Path(folder)/'unused.exe',Path(folder)/'work')
            campaign.repeat_after_failures=1
            campaign.planner_asset=dict(sha256='a'*64,bytes=12)
            slot=dict(id='synthetic-a',title='Synthetic',source=backend.descriptor(source),source_lock={},
                duration_seconds=120,caption='An invented test brief',seed_base=100)
            with patch.object(campaign,'locate',return_value=source):request=campaign.request(slot,6)
            backend.validate_request(request,source)
            self.assertEqual(request['semantic_planner'],campaign.planner_asset)
            self.assertNotIn('performance_repetitions',request)
            self.assertEqual(request['duration_seconds'],135)

    def test_continuation_formatting_preserves_words_and_stanza_boundaries(self):
        source='Amber lamps\r\nshine\r\n\r\nCopper\r\n'
        rendered=backend.format_lyrics(source,True)
        self.assertIn('Amber lamps shine',rendered)
        self.assertIn('[Verse]\nCopper',rendered)
        import re
        body=re.sub(r'^\s*\[[^\]\n]+\]\s*$','',rendered,flags=re.M)
        self.assertEqual(backend.words(body),backend.words(source))

    def test_lines_in_different_repeated_passes_are_not_discarded(self):
        lyrics='Amber lamps shine\nSilver bells ring\nCopper wheels turn'
        r=backend.line_report(lyrics,evidence('Silver bells ring copper wheels turn amber lamps shine silver bells ring'))
        self.assertTrue(r['passed'])
        self.assertEqual(r['exact_lines'],3)
        self.assertFalse(r['continuous_order_passed'])

    def test_scattered_words_are_not_a_line(self):
        r=backend.line_report('Amber silver copper',evidence('Amber one two three four five six copper'))
        self.assertFalse(r['passed'])

    def test_repeated_lines_do_not_penalize_complete_source(self):
        r=backend.line_report('Alpha beta gamma\nSilver bells ring',evidence('Alpha beta gamma alpha beta gamma silver bells ring'))
        self.assertTrue(r['passed'])
        self.assertEqual(r['exact_lines'],2)

    def test_one_word_gap_allowed_but_absent_single_word_line_fails(self):
        self.assertTrue(backend.line_report('Alpha beta gamma',evidence('Alpha beta'))['passed'])
        r=backend.line_report('Alpha beta gamma\nOmega\nSmall bells turn',evidence('Alpha beta gamma small bells turn'))
        self.assertFalse(r['passed'])
        self.assertFalse(r['lines'][1]['passes_gate'])

    def test_missing_line_and_two_word_gap_fail(self):
        self.assertFalse(backend.line_report('Alpha beta gamma',evidence('Beta'))['passed'])
        r=backend.line_report('Alpha beta gamma\nSilver bells ring',evidence('Alpha beta gamma'))
        self.assertFalse(r['passed'])
        self.assertEqual(r['qualifying_lines'],1)

    def test_repeated_source_lines_need_distinct_observed_words(self):
        r=backend.line_report('Bright bells ring\nBright bells ring',evidence('Bright bells ring'))
        self.assertFalse(r['passed'])

    def test_best_complete_pass_not_word_union(self):
        passes=evidence('Alpha beta')+evidence('gamma')
        self.assertFalse(backend.line_report('Alpha beta gamma delta',passes)['passed'])

    def test_pinned_missing_and_changed_files_fail(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'model';p.write_bytes(b'original');desc=backend.descriptor(p)
            self.assertEqual(backend.checked(p,desc),p)
            p.write_bytes(b'changed!')
            with self.assertRaises(ValueError):backend.checked(p,desc)
            p.unlink()
            with self.assertRaises(ValueError):backend.checked(p,desc)

    def test_unsupported_request_and_rewritten_lyrics_fail_before_generation(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'source.txt';p.write_bytes(b'Alpha beta gamma\n')
            request=dict(schema='reel.local-song-request.v1',task='cover',language='es',duration_seconds=60,seed=1,
                canonical_lyrics='Alpha beta gamma\n',render_lyrics='[Verse]\nAlpha beta gamma',
                permissions=dict(private_only=True,external_upload=False,identifiable_imitation=False,public_release=False))
            with self.assertRaises(ValueError):backend.validate_request(request,p)
            request['task']='text2music';backend.validate_request(request,p)
            request['recognition_dtype']='float16';backend.validate_request(request,p)
            request['recognition_dtype']='unsupported'
            with self.assertRaises(ValueError):backend.validate_request(request,p)
            request['recognition_dtype']='float32'
            request['render_lyrics']='Alpha beta delta'
            with self.assertRaises(ValueError):backend.validate_request(request,p)
            request['render_lyrics']='Alpha beta gamma';request['permissions']['external_upload']=True
            with self.assertRaises(ValueError):backend.validate_request(request,p)

    def test_authorized_full_repetition_preserves_canonical_source(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'source.txt';p.write_bytes(b'Alpha beta gamma\n')
            request=dict(schema='reel.local-song-request.v1',task='text2music',language='es',duration_seconds=60,seed=1,
                canonical_lyrics='Alpha beta gamma\n',render_lyrics='[Verse]\nAlpha beta gamma\n[Verse]\nAlpha beta gamma',
                performance_repetitions=2,
                permissions=dict(private_only=True,external_upload=False,identifiable_imitation=False,public_release=False))
            backend.validate_request(request,p)
            request['performance_repetitions']=1
            with self.assertRaises(ValueError):backend.validate_request(request,p)


if __name__=='__main__':unittest.main()
