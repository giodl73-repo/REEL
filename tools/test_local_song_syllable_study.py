import copy
import unittest
from local_song_syllable_study import syllables, choose_phrases, char_spans, derived_model, gap_phrases, chronological_text


class SyllableStudyTests(unittest.TestCase):
    def test_gap_scopes_preserve_previous_phrases_and_owner_omissions(self):
        report=dict(complete_canonical_lines=['a','b','c'],owner_accepted_omissions=[3],phrases=[],listening_windows=[])
        base=dict(phrases=[dict(id='left',source_span_seconds=[1,2]),dict(id='right',source_span_seconds=[5,6])])
        window=dict(line=2,span_seconds=[2,5],basis='neighbor-scope',left_anchor_id='left',right_anchor_id='right')
        result=gap_phrases(report,base,'a\nb\nc\n',[window],10)
        self.assertEqual(result[0]['performed_source_line'],'b')
        self.assertFalse(result[0]['independent_presence_test'])
        for changed in [dict(window,line=3),dict(window,span_seconds=[1.9,5]),dict(window,right_anchor_id='missing')]:
            with self.subTest(changed=changed),self.assertRaises(ValueError):
                gap_phrases(report,base,'a\nb\nc\n',[changed],10)
        base['syllables']=[dict(span_seconds=[1.9,2.1],note_ids=['retained-note'])]
        with self.assertRaises(ValueError):gap_phrases(report,base,'a\nb\nc\n',[window],10)

    def test_owner_scope_requires_real_confirmation_and_bounded_clock(self):
        report=dict(complete_canonical_lines=['hello'],owner_accepted_omissions=[],phrases=[],listening_windows=[dict(line=1,span_seconds=[3,4])])
        window=dict(line=1,span_seconds=[2.9,4.1],basis='owner-confirmed-presence')
        with self.assertRaises(ValueError):gap_phrases(report,dict(phrases=[]),'hello',[window],10)
        self.assertEqual(len(gap_phrases(report,dict(phrases=[]),'hello',[window],10,confirmed=[1])),1)
        with self.assertRaises(ValueError):gap_phrases(report,dict(phrases=[]),'hello',[dict(window,span_seconds=[1,5])],10,confirmed=[1])

    def test_inserted_phrase_rebases_utf8_without_changing_clock_or_text(self):
        phrases=[dict(id='later',performed_source_line='corazón',source_span_seconds=[4,5]),
                 dict(id='inserted',performed_source_line='sí',source_span_seconds=[2,3])]
        text='corazón\nsí\n';offset=len('corazón\n'.encode())
        rows=[dict(phrase_id='later',text='corazón',text_start_byte=0,text_end_byte=len('corazón'.encode()),span_seconds=[4,5]),
              dict(phrase_id='inserted',text='sí',text_start_byte=offset,text_end_byte=offset+len('sí'.encode()),span_seconds=[2,3])]
        ordered,newrows,newtext=chronological_text(phrases,rows,text)
        self.assertEqual(newtext,'sí\ncorazón\n')
        self.assertEqual([r['span_seconds'] for r in newrows],[[2,3],[4,5]])
        for row in newrows:self.assertEqual(newtext.encode()[row['text_start_byte']:row['text_end_byte']].decode(),row['text'])
        with self.assertRaises(ValueError):chronological_text(phrases,rows,text+'tampered')

    def test_orthographic_proposals_preserve_characters_and_common_groups(self):
        examples={'cielo':['cie','lo'],'copió':['co','pió'],'bosques':['bos','ques'],
                  'pretensiones':['pre','ten','sio','nes'],'sueño':['sue','ño'],
                  'huraña':['hu','ra','ña'],'María':['Ma','rí','a'],'cuídate':['cuí','da','te'],
                  'quieren':['quie','ren'],'país':['pa','ís'],'pajarillos':['pa','ja','ri','llos'],
                  'colores':['co','lo','res'],'retrato':['re','tra','to'],'hay':['hay']}
        for text,expected in examples.items():
            with self.subTest(text=text):
                actual=[text[a:b] for a,b in syllables(text)]
                self.assertEqual(actual,expected)
                self.assertEqual(''.join(actual),text)

    def test_repeated_ctc_letter_requires_blank_and_retains_target_order(self):
        self.assertEqual(char_spans([1,1,0,1,2], 'aab', ['-','a','b'], .1, 2),
                         [[2,2.2],[2.3,2.4],[2.4,2.5]])
        with self.assertRaises(ValueError):char_spans([1,1,2],'aab',['-','a','b'],.1,0)

    def test_schedule_retains_repeats_excludes_alternatives_and_omissions(self):
        def phrase(id,line,start,end):
            return dict(id=id,line=line,span_seconds=[start,end],words=[dict(token_index=0,source_span_seconds=[start,end],note_ids=['n'],status='shared-pitch-run')])
        report=dict(owner_accepted_omissions=[3],phrases=[phrase('a',1,1,2),phrase('alternative',1,1.1,2.1),phrase('repeat',1,3,4),phrase('forbidden',3,5,6)])
        result=choose_phrases(report)
        self.assertEqual(len(result),2)
        self.assertEqual(result[-1]['id'],'repeat')
        self.assertNotIn('forbidden',[r['id'] for r in result])

    def test_study_intersections_preserve_parent_pitches_and_parent_model(self):
        parent=dict(model_id='parent',tempo_map=[dict(tick=0,microseconds_per_quarter=1000000)],
                    musical_timebase=dict(pulses_per_quarter=100),parts=[dict(notes=[
                        dict(id='parent-1',voice=1,start_tick=100,duration_ticks=200,midi_note=60,velocity=72,provenance=dict(rationale='original')),
                        dict(id='uncovered',voice=1,start_tick=500,duration_ticks=100,midi_note=64,velocity=72,provenance=dict(rationale='original'))])],
                    authority=dict(status='draft-not-reviewed',decision_refs=[]),review=dict(status='not-reviewed',decision_refs=[]),
                    lyric_layers=[],unknowns=[],expressive_timing=[])
        original=copy.deepcopy(parent)
        rows=[dict(span_seconds=[1.2,2],mean_log_probability=-1,text_start_byte=0,text_end_byte=2,syllabic='begin'),
              dict(span_seconds=[2,2.8],mean_log_probability=-1,text_start_byte=2,text_end_byte=4,syllabic='end')]
        model,ledger=derived_model(parent,rows,'casa','Study')
        self.assertEqual(parent,original)
        self.assertEqual([n['midi_note'] for n in model['parts'][0]['notes']],[60,60])
        self.assertEqual([n['duration_ticks'] for n in model['parts'][0]['notes']],[80,80])
        self.assertEqual({n['parent_note_id'] for n in ledger},{'parent-1'})
        self.assertEqual(len(model['lead_sheet']['underlay']),2)
        self.assertIn('PARTIAL',model['lead_sheet']['title'])


if __name__=='__main__':unittest.main()
