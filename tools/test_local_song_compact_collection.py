import json
import math
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from local_song_backend import descriptor
from local_song_album_master import decoded
from local_song_compact_collection import build,choose_bitrate


class CompactCollectionTests(unittest.TestCase):
    def test_budget_keeps_listening_floor(self):
        self.assertEqual(choose_bitrate(16773137,6131.686125,120000000),128000)
        with self.assertRaises(ValueError):choose_bitrate(80000000,6131,120000000)

    def test_real_fractional_chapter_joins_copy_every_video_packet(self):
        ff=Path('D:/tools/bin/ffmpeg.exe');fp=Path('D:/tools/bin/ffprobe.exe')
        if not ff.exists() or not fp.exists():self.skipTest('Explicit local fixture codecs unavailable')
        previous=Path.cwd()
        with tempfile.TemporaryDirectory() as folder:
            os.chdir(folder)
            try:
                chapters=[]
                for i,(duration,color) in enumerate([(2.3,'tan'),(2.47,'teal')],1):
                    audio=f'{i}.mp3';video=f'{i}.mp4'
                    subprocess.run([str(ff),'-v','error','-f','lavfi','-i',f'sine=frequency={i*330}:sample_rate=48000:duration={duration}','-ac','2','-c:a','libmp3lame','-b:a','320k',audio],check=True)
                    subprocess.run([str(ff),'-v','error','-f','lavfi','-i',f'color=c={color}:s=480x270:r=5:d={math.ceil(duration*5)/5}','-i',audio,'-c:v','libx264','-pix_fmt','yuv420p','-c:a','copy','-video_track_timescale','48000',video],check=True)
                    pcm=decoded(ff,audio,'f32le',8);chapters.append(dict(id=str(i),title=f'Chapter {i}',video=dict(path=video,**descriptor(video)),audio=dict(path=audio,**descriptor(audio)),decoded_audio=pcm))
                q=dict(schema='reel.compact-chaptered-collection.v1',title='Collection',permissions=dict(private_only=True,external_upload=False,public_release=False),ffmpeg=dict(path=str(ff),**descriptor(ff)),ffprobe=dict(path=str(fp),**descriptor(fp)),chapters=chapters,maximum_bytes=2000000,audio_bitrate=128000)
                Path('collection-request.json').write_text(json.dumps(q),encoding='utf-8');build(q)
                report=json.loads(Path('collection-check.json').read_text(encoding='utf-8'))
                self.assertTrue(report['all_video_packet_payloads_and_pts_exact']);self.assertEqual(report['chapters'][1]['start_frame'],110400)
                self.assertTrue(report['full_decode_verified']);self.assertLess(report['movie']['bytes'],2000000)
                self.assertEqual(report['audio_codec'],'aac');self.assertTrue(report['audio_reencoded'])
                q['maximum_bytes']=10000
                with self.assertRaises(ValueError):choose_bitrate(1000,report['seconds'],q['maximum_bytes'])
            finally:os.chdir(previous)


if __name__=='__main__':unittest.main()
