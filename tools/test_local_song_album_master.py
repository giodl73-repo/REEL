import contextlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
import wave

import numpy as np
from local_song_backend import descriptor
from local_song_album_master import build, gain_db, validate


class AlbumTests(unittest.TestCase):
    def request(self):
        return dict(schema='reel.local-song-album-master.v1', title='Álbum de prueba',
            delivery_stem='Álbum de prueba', gap_seconds=2, target_lufs=-16,
            source_peak_ceiling_dbtp=-2, permissions=dict(private_only=True, external_upload=False, public_release=False),
            tracks=[dict(position=i+1, song_id=str(i), title='Canción '+str(i)) for i in range(2)])

    def test_order_scope_and_peak_cap(self):
        q=self.request();validate(q)
        q['tracks'][1]['song_id']='0'
        with self.assertRaises(ValueError):validate(q)
        q=self.request();q['permissions']['external_upload']=True
        with self.assertRaises(ValueError):validate(q)
        self.assertEqual(gain_db(dict(input_i='-25',input_tp='-3'),-16,-2),1)
        self.assertEqual(gain_db(dict(input_i='-12',input_tp='-1'),-16,-2),-4)

    def test_complete_tail_gap_unicode_and_lossless_join(self):
        ff=Path('D:/tools/bin/ffmpeg.exe')
        if not ff.exists():self.skipTest('Explicit installed fixture FFmpeg unavailable')
        previous=Path.cwd()
        with tempfile.TemporaryDirectory() as folder:
            os.chdir(folder)
            try:
                q=self.request();q['ffmpeg']=dict(path=str(ff),**descriptor(ff))
                runtime=dict(python=descriptor(sys.executable),files=[dict(path=np.__file__,**descriptor(np.__file__))])
                Path('runtime.json').write_text(json.dumps(runtime),encoding='utf-8')
                q['numeric_runtime']=dict(path='runtime.json',**descriptor('runtime.json'))
                for i,track in enumerate(q['tracks']):
                    # Distinct pitches and a late final pulse catch truncation,
                    # order mistakes and a missing complete last section.
                    rate=44100;count=rate*5;t=np.arange(count)/rate
                    y=.09*np.sin(2*np.pi*(330+110*i)*t)
                    y[-rate//4:]+=.025*np.sin(2*np.pi*880*t[-rate//4:])
                    audio=np.rint(np.stack([y,y],axis=1)*32767).astype('<i2')
                    name=f'{i}.wav'
                    with wave.open(name,'wb') as out:
                        out.setparams((2,2,rate,0,'NONE','not compressed'));out.writeframes(audio.tobytes())
                    track.update(audio=dict(path=name,**descriptor(name)),golden=dict(logical_id='fixture/'+str(i),version_id='v1',manifest_sha256='0'*64))
                Path('album-request.json').write_text(json.dumps(q),encoding='utf-8');build(q)
                check=json.loads(Path('master-check.json').read_text(encoding='utf-8'))
                self.assertEqual(check['frames'],12*48000)
                self.assertEqual([s['start_frame'] for s in check['tracks']],[0,7*48000])
                self.assertEqual([s['frames'] for s in check['tracks']],[5*48000]*2)
                self.assertNotEqual(check['tracks'][0]['mastered_pcm24_sha256'],check['tracks'][1]['mastered_pcm24_sha256'])
                self.assertTrue(check['lossless_pcm_exact'])
                self.assertIn('Álbum de prueba.flac',Path('chapters.cue').read_text(encoding='utf-8'))
            finally:os.chdir(previous)


if __name__=='__main__':unittest.main()
