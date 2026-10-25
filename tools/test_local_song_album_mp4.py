import json
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import numpy as np

from local_song_backend import descriptor
from local_song_album_mp4 import build, reading_labels, wrapped


class AlbumMp4Tests(unittest.TestCase):
    def test_phone_page_bounds_and_missing_translation(self):
        check=dict(title='Álbum',duration_seconds=6,sample_rate=48000,tracks=[dict(song_id='one',title='Uno',start_frame=0)])
        palette=dict(paper='#F5EAD4',ink='#3D2B24',accent='#2C6462',secondary='#A95F46',rule='#C4AE87')
        plan=dict(schema='reel.album-bilingual-reading.v1',mode='editorial-reading-pages',palette=palette,tracks=[dict(song_id='one',mode='sung',pages=[dict(start_frame=0,end_frame=288000,lines=[dict(es='Una palabra',en='One word')]*8)])])
        with self.assertRaises(ValueError):reading_labels(plan,check)
        with self.assertRaises(ValueError):wrapped(None)
        plan['tracks'][0]['pages'][0]['lines']=[dict(es='Una palabra',en='One word')]
        plan['tracks'][0]['pages'][0]['end_frame']=336000
        with self.assertRaises(ValueError):reading_labels(plan,check)

    def test_real_export_copies_all_audio_packets_and_chapters(self):
        ff=Path('D:/tools/bin/ffmpeg.exe');fp=Path('D:/tools/bin/ffprobe.exe');font=Path('C:/Windows/Fonts/georgia.ttf')
        if not all(p.exists() for p in [ff,fp,font]):self.skipTest('Explicit local fixture tools unavailable')
        previous=Path.cwd()
        with tempfile.TemporaryDirectory() as folder:
            os.chdir(folder)
            try:
                subprocess.run([str(ff),'-v','error','-f','lavfi','-i','sine=frequency=330:sample_rate=48000:duration=6','-ac','2','-c:a','libmp3lame','-b:a','320k','album.mp3'],check=True)
                pcm=subprocess.check_output([str(ff),'-v','error','-i','album.mp3','-ar','48000','-ac','2','-f','f32le','-'])
                check=dict(title='Prueba del sueño',duration_seconds=6,sample_rate=48000,mp3_decode=dict(frames=len(pcm)//8,sha256=hashlib.sha256(pcm).hexdigest()),tracks=[dict(song_id='one',title='Primero {uno}',start_frame=0),dict(song_id='two',title='Después',start_frame=144000)])
                Path('album-check.json').write_text(json.dumps(check),encoding='utf-8')
                Path('chapters.ffmetadata').write_text(';FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/48000\nSTART=0\nEND=144000\ntitle=Primero\n[CHAPTER]\nTIMEBASE=1/48000\nSTART=144000\nEND=288000\ntitle=Después\n',encoding='utf-8')
                q=dict(schema='reel.local-song-album-mp4.v1',title=check['title'],permissions=dict(private_only=True,external_upload=False,public_release=False),ffmpeg=dict(path=str(ff),**descriptor(ff)),ffprobe=dict(path=str(fp),**descriptor(fp)),font=dict(path=str(font),**descriptor(font)),audio=descriptor('album.mp3'),chapters=descriptor('chapters.ffmetadata'),album_check=descriptor('album-check.json'))
                plan=dict(schema='reel.album-bilingual-reading.v1',mode='editorial-reading-pages',source_album_check=q['album_check'],palette=dict(paper='#F5EAD4',ink='#3D2B24',accent='#2C6462',secondary='#A95F46',rule='#C4AE87'),tracks=[dict(song_id='one',mode='sung',pages=[dict(start_frame=0,end_frame=144000,lines=[dict(es='Un recuerdo delicado de una mañana junto al mar de Cuba',en='A delicate memory of the quiet and beautiful Cuban countryside'),dict(es='La palma canta',en='The palm sings')])]),dict(song_id='two',mode='instrumental',pages=[])])
                Path('reading.json').write_text(json.dumps(plan),encoding='utf-8');q['reading_plan']=dict(path='reading.json',**descriptor('reading.json'))
                Path('mp4-request.json').write_text(json.dumps(q),encoding='utf-8');build(q)
                result=json.loads(Path('mp4-check.json').read_text(encoding='utf-8'))
                self.assertTrue(result['all_audio_packet_payloads_exact']);self.assertTrue(result['full_decode']);self.assertEqual(result['track_count'],2)
                self.assertGreater(result['audio_packet_count'],200)
                self.assertIn('Primero (uno)',Path('labels.ass').read_text(encoding='utf-8'))
                self.assertIn('Cuban countryside',Path('labels.ass').read_text(encoding='utf-8'))
                self.assertEqual(result['video']['height'],1080)
                self.assertEqual(result['decoded_pcm_identity'],check['mp3_decode'])
                # Inspect rendered pixels without displaying a screenshot: the
                # two columns must each contain four separated text rows.
                raw=subprocess.check_output([str(ff),'-v','error','-ss','1','-i','master.mp4','-frames:v','1','-f','rawvideo','-pix_fmt','rgb24','-'])
                frame=np.frombuffer(raw,dtype='u1').reshape(1080,1920,3)
                for left in [118,1010]:
                    crop=frame[335:932,left:left+792]
                    ink=(crop[:,:,0]<100)&(crop[:,:,1]<90)&(crop[:,:,2]<80)
                    occupied=np.flatnonzero(ink.sum(axis=1)>3)
                    # Thin descender/diacritic strokes can fall below the ink
                    # threshold inside one line; merge those small islands.
                    groups=np.split(occupied,np.where(np.diff(occupied)>12)[0]+1)
                    self.assertEqual(len(groups),4,[(int(g[0]),int(g[-1])) for g in groups])
                    self.assertTrue(all(b[0]-a[-1]>8 for a,b in zip(groups,groups[1:])))
            finally:os.chdir(previous)

    def test_performed_cards_render_before_phrases_and_clear_between_them(self):
        ff=Path('D:/tools/bin/ffmpeg.exe');fp=Path('D:/tools/bin/ffprobe.exe');font=Path('C:/Windows/Fonts/georgia.ttf')
        if not all(p.exists() for p in [ff,fp,font]):self.skipTest('Explicit local fixture tools unavailable')
        previous=Path.cwd()
        with tempfile.TemporaryDirectory() as folder:
            os.chdir(folder)
            try:
                subprocess.run([str(ff),'-v','error','-f','lavfi','-i','sine=frequency=330:sample_rate=48000:duration=3','-ac','2','-c:a','libmp3lame','album.mp3'],check=True)
                pcm=subprocess.check_output([str(ff),'-v','error','-i','album.mp3','-ar','48000','-ac','2','-f','f32le','-'])
                check=dict(title='Fixture',duration_seconds=3,sample_rate=48000,mp3_decode=dict(frames=len(pcm)//8,sha256=hashlib.sha256(pcm).hexdigest()),tracks=[dict(song_id='one',title='One',start_frame=0)])
                Path('album-check.json').write_text(json.dumps(check),encoding='utf-8');Path('chapters.ffmetadata').write_text(';FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/48000\nSTART=0\nEND=144000\ntitle=One\n',encoding='utf-8')
                q=dict(schema='reel.local-song-album-mp4.v1',title='Fixture',permissions=dict(private_only=True,external_upload=False,public_release=False),ffmpeg=dict(path=str(ff),**descriptor(ff)),ffprobe=dict(path=str(fp),**descriptor(fp)),font=dict(path=str(font),**descriptor(font)),audio=descriptor('album.mp3'),chapters=descriptor('chapters.ffmetadata'),album_check=descriptor('album-check.json'))
                pages=[dict(start_frame=28800,end_frame=67200,lines=[dict(es='Primera frase',en='First phrase')],phrase_evidence=[dict(start=.8,end=1.3)]),dict(start_frame=105600,end_frame=134400,lines=[dict(es='Segunda frase',en='Second phrase')],phrase_evidence=[dict(start=2.4,end=2.7)])]
                plan=dict(schema='reel.album-bilingual-reading.v1',mode='performed-phrase-pages',source_album_check=q['album_check'],palette=dict(paper='#F5EAD4',ink='#3D2B24',accent='#2C6462',secondary='#A95F46',rule='#C4AE87'),tracks=[dict(song_id='one',mode='sung',pages=pages)])
                Path('reading.json').write_text(json.dumps(plan),encoding='utf-8');q['reading_plan']=dict(path='reading.json',**descriptor('reading.json'));Path('mp4-request.json').write_text(json.dumps(q),encoding='utf-8');build(q)
                for timestamp,visible in [(.4,False),(.6,True),(1.8,False),(2.2,True)]:
                    raw=subprocess.check_output([str(ff),'-v','error','-ss',str(timestamp),'-i','master.mp4','-frames:v','1','-f','rawvideo','-pix_fmt','rgb24','-'])
                    frame=np.frombuffer(raw,dtype='u1').reshape(1080,1920,3)
                    for left in [118,1010]:
                        crop=frame[335:440,left:left+792];ink=(crop[:,:,0]<100)&(crop[:,:,1]<90)&(crop[:,:,2]<80)
                        self.assertEqual(int(ink.sum())>500,visible,(timestamp,left,int(ink.sum())))
            finally:os.chdir(previous)


if __name__=='__main__':unittest.main()
