import unittest
import numpy as np
from local_song_timing_conform import compare
from local_song_lyric_pages import event_occurrences, timed_pages
from local_song_album_mp4 import reading_labels, wrapped


class LyricClockTests(unittest.TestCase):
    def test_recording_offset_and_wrong_performance(self):
        rng=np.random.default_rng(4);source=rng.normal(size=40000).astype('f4');target=np.concatenate([np.zeros(100),source])
        result=compare(source,target)
        self.assertTrue(result['transferable']);self.assertAlmostEqual(result['offset_seconds'],.05)
        self.assertFalse(compare(source,np.concatenate([source,np.zeros(8000)]))['transferable'])
        self.assertFalse(compare(source,rng.normal(size=40000).astype('f4'))['transferable'])

    def test_repeats_and_interludes_follow_audio_entrances(self):
        lines=[dict(es='Primera frase',en='First phrase'),dict(es='Segunda frase',en='Second phrase')]
        events=[dict(text='Primera frase',start=5.,end=8.),dict(text='Segunda frase',start=9.,end=12.),dict(text='Primera frase',start=30.,end=33.)]
        rows=event_occurrences(lines,events);found=[x for row in rows for x in row['occurrences']]
        pages,accepted=timed_pages(found,lines,100,140,44100,wrapped)
        self.assertEqual(len(accepted),3);self.assertEqual(len(pages),2)
        self.assertAlmostEqual(pages[1]['start_frame']/44100,129.6)
        self.assertLess(pages[0]['end_frame']/44100,115)
        check=dict(title='Album',duration_seconds=140,sample_rate=44100,tracks=[dict(song_id='one',title='One',start_frame=4410000)])
        plan=dict(schema='reel.album-bilingual-reading.v1',mode='performed-phrase-pages',palette=dict(paper='#F5EAD4',ink='#3D2B24',accent='#2C6462',secondary='#A95F46',rule='#C4AE87'),tracks=[dict(song_id='one',mode='sung',pages=pages)])
        labels,ledger=reading_labels(plan,check);self.assertIn('0:02:09.60',labels)
        pages[1]['start_frame']=round(131*44100)
        with self.assertRaisesRegex(ValueError,'misses its performed phrase'):reading_labels(plan,check)

    def test_canonical_context_keeps_unrecognized_words_without_inventing_an_onset(self):
        lines=[dict(es='Apertura sin reloj',en='Opening without a clock'),dict(es='La frase medida',en='The measured phrase'),dict(es='Contexto conservado',en='Retained context')]
        found=[dict(line=2,start=15.,end=18.,score=1.)]
        pages,accepted=timed_pages(found,lines,0,30,48000,wrapped,preserve_context=True)
        self.assertEqual([p['es'] for p in pages[0]['lines']],[p['es'] for p in lines])
        self.assertEqual(len(pages[0]['phrase_evidence']),1)
        self.assertEqual(pages[0]['start_frame'],11*48000)
        self.assertEqual(len([p for p in pages[0]['lines'] if p.get('timing_status')]),2)

    def test_missing_words_get_no_invented_clock(self):
        rows=event_occurrences([dict(es='Una frase completamente ausente',en='A missing phrase')],[dict(text='otro',start=1.,end=2.)])
        self.assertEqual(rows[0]['occurrences'],[])


if __name__=='__main__':unittest.main()
