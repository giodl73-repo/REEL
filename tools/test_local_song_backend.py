"""Synthetic opaque-token contract fixtures; no private lyrics or model/audio."""
import tempfile
import os
from pathlib import Path
import unittest
import local_song_backend as backend
import local_song_campaign  # Campaign syntax/import must remain dependency-free.
import local_song_ctc
import json


def evidence(text):
    return [dict(name='synthetic',tokens=[dict(word=w,span=[i,i+1]) for i,w in enumerate(backend.words(text))])]


class LyricGateTests(unittest.TestCase):
    def test_bounded_serial_round_advances_other_slots_without_claiming_completion(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);work=root/'work';campaign=local_song_campaign.Campaign(root,root/'unused.exe',work)
            path=campaign.save('campaign.json',dict(schema='reel.song-audition-campaign.v1',slots=[dict(id='first'),dict(id='second')]))
            runtime=campaign.save('runtime.json',dict(sha256='synthetic',bytes=0))
            def result(slot,attempt):return dict(id=slot['id']+'-'+str(attempt),slot_id=slot['id'],attempt=attempt,qualified=slot['id']=='second' or attempt>1)
            with patch.object(campaign,'ingest',return_value=dict(sha256='synthetic',bytes=0)),patch.object(campaign,'locate',return_value=runtime),patch.object(campaign,'review'),patch.object(campaign,'run_take',side_effect=result) as take:
                campaign.run(path,runtime,root/'backend.py',takes_per_slot=1)
                active=json.loads((work/'active.json').read_text(encoding='utf-8'))
                self.assertEqual(active,dict(stage='round-complete',qualified=1,target=2))
                self.assertEqual([x.args[0]['id'] for x in take.call_args_list],['first','second'])
                campaign.run(path,runtime,root/'backend.py',takes_per_slot=1)
                active=json.loads((work/'active.json').read_text(encoding='utf-8'))
                self.assertEqual(active,dict(stage='complete',qualified=2,target=2))
                state=json.loads((work/'state.json').read_text(encoding='utf-8'))
                self.assertEqual(state['qualified']['second']['attempt'],1)
                self.assertEqual(len(state['attempts']),3)

    def test_scoped_guide_cannot_target_another_source_or_unknown_slot(self):
        request=dict(source={'sha256':'synthetic-one'},source_lock={'version_id':'v1'})
        same=dict(id='same',**request)
        other=dict(id='other',source={'sha256':'synthetic-two'},source_lock={'version_id':'v1'})
        self.assertEqual(local_song_campaign.guide_targets([same,other],request,['same','same']),['same'])
        for targets in [[],['unknown'],['other']]:
            with self.assertRaises(ValueError):local_song_campaign.guide_targets([same,other],request,targets)

    def test_scoped_guide_leaves_unrelated_slot_as_original_text2music(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as folder:
            source=Path(folder)/'source.txt';source.write_text('Alpha beta gamma\n')
            campaign=local_song_campaign.Campaign(folder,Path(folder)/'unused.exe',Path(folder)/'work')
            campaign.audio_guide=dict(target_slot_ids=['other'],request={'not':'resolved'},recording={},generation={},version_lock={},label='A',strength=.75)
            slot=dict(id='synthetic-short',title='Synthetic',source={},source_lock={'different':'version'},duration_seconds=90,max_duration_seconds=90,caption='Invented brief',seed_base=100)
            with patch.object(campaign,'locate',return_value=source) as locate:request=campaign.request(slot,1)
            locate.assert_called_once_with(slot['source'])
            self.assertEqual(request['task'],'text2music');self.assertNotIn('guide_audio',request)
            self.assertEqual(request['duration_seconds'],90)

    def test_duration_ceiling_survives_retries_and_explicit_repetition(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as folder:
            source=Path(folder)/'source.txt';source.write_text('Alpha beta gamma\nDelta echo foxtrot\n')
            campaign=local_song_campaign.Campaign(folder,Path(folder)/'unused.exe',Path(folder)/'work')
            slot=dict(id='synthetic-short',title='Synthetic',source={},source_lock={},duration_seconds=90,max_duration_seconds=105,caption='Invented brief',seed_base=100)
            with patch.object(campaign,'locate',return_value=source):
                self.assertEqual(campaign.request(slot,1)['duration_seconds'],90)
                self.assertEqual(campaign.request(slot,10)['duration_seconds'],105)
                campaign.repeat_after_failures=2
                self.assertEqual(campaign.request(slot,10)['duration_seconds'],105)
                slot['max_duration_seconds']=89
                with self.assertRaises(ValueError):campaign.request(slot,1)

    def test_duration_ceiling_rejects_longer_timing_reference(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as folder:
            source=Path(folder)/'source.txt';source.write_text('Alpha beta gamma\n')
            campaign=local_song_campaign.Campaign(folder,Path(folder)/'unused.exe',Path(folder)/'work')
            campaign.timing_reference=dict(source={},source_lock={},seed=10,duration_seconds=180,attempt_starts={'synthetic-short':1},request={},version_lock={})
            slot=dict(id='synthetic-short',title='Synthetic',source={},source_lock={},duration_seconds=90,max_duration_seconds=105,caption='Invented brief',seed_base=100)
            with patch.object(campaign,'locate',return_value=source):
                with self.assertRaises(ValueError):campaign.request(slot,1)

    def test_generated_guide_is_hash_bound_and_rejects_another_source_version(self):
        with tempfile.TemporaryDirectory() as folder:
            previous=Path.cwd()
            try:
                os.chdir(folder);source=Path('source.txt');source.write_bytes(b'Alpha beta gamma\n')
                original=dict(schema='reel.local-song-request.v1',task='text2music',language='es',duration_seconds=60,seed=1,
                    canonical_lyrics='Alpha beta gamma\n',render_lyrics='[Verse]\nAlpha beta gamma',source=backend.descriptor(source),source_lock={},
                    permissions=dict(private_only=True,external_upload=False,identifiable_imitation=False,public_release=False))
                backend.write('guide-request.json',original);Path('guide.wav').write_bytes(b'synthetic proof-binding fixture, not audio')
                backend.write('guide-generation.json',dict(request=backend.descriptor('guide-request.json'),recording=backend.descriptor('guide.wav')))
                request=dict(original,task='cover',guide_audio=backend.descriptor('guide.wav'),guide_request=backend.descriptor('guide-request.json'),
                    guide_generation=backend.descriptor('guide-generation.json'),guide_version_lock={},audio_cover_strength=.75)
                backend.validate_request(request,source)
                self.assertEqual(backend.validate_generated_guide(request,source)['recording'],backend.descriptor('guide.wav'))
                request['source_lock']={'wrong':'version'}
                with self.assertRaises(ValueError):backend.validate_generated_guide(request,source)
            finally:os.chdir(previous)

    def test_guided_candidate_preserves_source_render_and_distinct_brief(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as folder:
            source=Path(folder)/'source.txt';source.write_bytes(b'Alpha beta gamma\nDelta echo foxtrot\n')
            original=dict(task='text2music',canonical_lyrics=source.read_bytes().decode(),source_lock={},duration_seconds=60,
                render_lyrics=backend.format_repeated_sections(source.read_text(),2,12),performance_repetitions=2,repeat_section_lines=2,phrase_max_words=12)
            proof=Path(folder)/'original.json';backend.write(proof,original);proof_desc=backend.descriptor(proof)
            campaign=local_song_campaign.Campaign(folder,Path(folder)/'unused.exe',Path(folder)/'work')
            campaign.repeat_section_lines=4;campaign.phrase_max_words=12
            campaign.audio_guide=dict(request=proof_desc,recording={},generation={},version_lock={},label='A',strength=.75)
            slot=dict(id='synthetic-c',title='Synthetic',source=backend.descriptor(source),source_lock={},duration_seconds=120,caption='A distinct invented musical brief',seed_base=100)
            with patch.object(campaign,'locate',side_effect=lambda asset:proof if asset==proof_desc else source):request=campaign.request(slot,3)
            self.assertEqual(request['task'],'cover');self.assertEqual(request['duration_seconds'],60)
            self.assertEqual(request['render_lyrics'],original['render_lyrics']);self.assertEqual(request['repeat_section_lines'],2)
            self.assertTrue(request['caption'].startswith(slot['caption']));backend.validate_request(request,source)

    def test_independent_ctc_uses_bounded_neighboring_time_windows(self):
        report=dict(passed=False,technical=dict(seconds=100),lines=[
            dict(passes_gate=True,recognized_span=[10,15]),dict(passes_gate=False,recognized_span=None),
            dict(passes_gate=True,recognized_span=[24,28])])
        self.assertEqual(local_song_ctc.uncertain_windows(report),[[6,32]])

    def test_independent_ctc_skips_qualified_or_far_from_pass_reports(self):
        self.assertEqual(local_song_ctc.uncertain_windows(dict(passed=True,lines=[])),[])
        self.assertEqual(local_song_ctc.uncertain_windows(dict(passed=False,lines=[dict(passes_gate=False)]*4)),[])

    def test_independent_ctc_keeps_real_repeats_and_word_times(self):
        tokens=local_song_ctc.decode_ids([0,2,2,0,2,1,3,0,4,0],['-','|','A','B','C'],.1,10)
        self.assertEqual([t['word'] for t in tokens],['aa','bc'])
        self.assertAlmostEqual(tokens[0]['span'][0],10.1)
        self.assertAlmostEqual(tokens[0]['span'][1],10.5)
        self.assertAlmostEqual(tokens[1]['span'][1],10.9)

    def test_independent_ctc_blank_audio_invents_no_words(self):
        self.assertEqual(local_song_ctc.decode_ids([0,0,1,1,0],['-','|','A'],.1,0),[])

    def test_generation_focus_quotes_only_original_source_line(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as folder:
            source=Path(folder)/'source.txt';source.write_text('Amber lamps shine\nSilver bells ring\n',encoding='utf-8')
            campaign=local_song_campaign.Campaign(folder,Path(folder)/'unused.exe',Path(folder)/'work')
            campaign.lyric_focus=dict(line_numbers=[2],analysis={},version_lock={},recognition_prompt=False)
            slot=dict(id='synthetic-b',title='Synthetic',source=backend.descriptor(source),source_lock={},duration_seconds=120,caption='An invented musical brief',seed_base=100)
            with patch.object(campaign,'locate',return_value=source):request=campaign.request(slot,2)
            self.assertIn("'Silver bells ring'",request['caption'])
            self.assertFalse(request['lyric_focus']['recognition_prompt'])
            self.assertEqual(request['canonical_lyrics'],source.read_bytes().decode('utf-8'))
            backend.validate_request(request,source)

    def test_timing_reference_keeps_brief_and_advances_retry_seed(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as folder:
            source=Path(folder)/'source.txt';source.write_text('Amber lamps shine\n',encoding='utf-8')
            campaign=local_song_campaign.Campaign(folder,Path(folder)/'unused.exe',Path(folder)/'work')
            desc=backend.descriptor(source);slot=dict(id='synthetic-b',title='Synthetic',source=desc,source_lock={},duration_seconds=120,caption='A different invented musical brief',seed_base=100)
            campaign.timing_reference=dict(source=desc,source_lock={},seed=700,duration_seconds=180,attempt_starts={'synthetic-b':3},request={},version_lock={})
            with patch.object(campaign,'locate',return_value=source):
                first=campaign.request(slot,3);second=campaign.request(slot,4)
                self.assertEqual((first['seed'],second['seed']),(700,701))
                self.assertEqual(first['duration_seconds'],180)
                self.assertTrue(first['caption'].startswith(slot['caption']))
                self.assertFalse(first['timing_seed_reference']['audio_conditioning'])
                backend.validate_request(first,source)
                slot['source_lock']={'wrong':'version'}
                with self.assertRaises(ValueError):campaign.request(slot,5)

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
