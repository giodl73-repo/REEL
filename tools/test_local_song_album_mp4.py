import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from local_song_backend import descriptor
from local_song_album_mp4 import build


class AlbumMp4Tests(unittest.TestCase):
    def test_real_export_copies_all_audio_packets_and_chapters(self):
        ff=Path('D:/tools/bin/ffmpeg.exe');fp=Path('D:/tools/bin/ffprobe.exe');font=Path('C:/Windows/Fonts/segoeui.ttf')
        if not all(p.exists() for p in [ff,fp,font]):self.skipTest('Explicit local fixture tools unavailable')
        previous=Path.cwd()
        with tempfile.TemporaryDirectory() as folder:
            os.chdir(folder)
            try:
                subprocess.run([str(ff),'-v','error','-f','lavfi','-i','sine=frequency=330:sample_rate=48000:duration=6','-ac','2','-c:a','libmp3lame','-b:a','320k','album.mp3'],check=True)
                check=dict(title='Prueba del sueño',duration_seconds=6,sample_rate=48000,tracks=[dict(title='Primero {uno}',start_frame=0),dict(title='Después',start_frame=144000)])
                Path('album-check.json').write_text(json.dumps(check),encoding='utf-8')
                Path('chapters.ffmetadata').write_text(';FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/48000\nSTART=0\nEND=144000\ntitle=Primero\n[CHAPTER]\nTIMEBASE=1/48000\nSTART=144000\nEND=288000\ntitle=Después\n',encoding='utf-8')
                q=dict(schema='reel.local-song-album-mp4.v1',title=check['title'],permissions=dict(private_only=True,external_upload=False,public_release=False),ffmpeg=dict(path=str(ff),**descriptor(ff)),ffprobe=dict(path=str(fp),**descriptor(fp)),font=dict(path=str(font),**descriptor(font)),audio=descriptor('album.mp3'),chapters=descriptor('chapters.ffmetadata'),album_check=descriptor('album-check.json'))
                Path('mp4-request.json').write_text(json.dumps(q),encoding='utf-8');build(q)
                result=json.loads(Path('mp4-check.json').read_text(encoding='utf-8'))
                self.assertTrue(result['all_audio_packet_payloads_exact']);self.assertTrue(result['full_decode']);self.assertEqual(result['track_count'],2)
                self.assertGreater(result['audio_packet_count'],200)
                self.assertIn('Primero (uno)',Path('labels.ass').read_text(encoding='utf-8'))
            finally:os.chdir(previous)


if __name__=='__main__':unittest.main()
