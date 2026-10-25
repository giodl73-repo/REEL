"""Offline full-recording album assembly with static gain and exact PCM joins.

Run as a declared native asset job. The request owns order, source locks,
permissions, album names and mastering settings; no discovery or uploads.
"""
import hashlib
import json
import math
import re
from pathlib import Path
import subprocess
import sys

from local_song_backend import checked, descriptor, offline, read, write


RATE = 48000


def validate(q):
    if q['schema'] != 'reel.local-song-album-master.v1':
        raise ValueError('Unsupported album request')
    if q['permissions'] != dict(private_only=True, external_upload=False, public_release=False):
        raise ValueError('Explicit private offline scope required')
    tracks = q['tracks']
    if not 1 <= len(tracks) <= 128 or len({t['song_id'] for t in tracks}) != len(tracks):
        raise ValueError('Expected unique ordered album tracks')
    if [t['position'] for t in tracks] != list(range(1, len(tracks)+1)):
        raise ValueError('Nonsequential track positions')
    if not 0 <= q['gap_seconds'] <= 5 or not -24 <= q['target_lufs'] <= -12 or not -6 <= q['source_peak_ceiling_dbtp'] <= -2:
        raise ValueError('Unsupported mastering bounds')
    for name in ['title', 'delivery_stem']:
        if not q[name] or any(c in q[name] for c in '\r\n/\\"'):
            raise ValueError('Unsafe album label')


def loudness(ff, path):
    result = subprocess.run([str(ff), '-v', 'info', '-nostats', '-nostdin', '-i', str(path),
        '-af', 'loudnorm=I=-16:TP=-2:LRA=11:print_format=json', '-f', 'null', '-'],
        capture_output=True, text=True, encoding='utf-8', errors='replace', check=True)
    start = result.stderr.rfind('{'); end = result.stderr.rfind('}')
    report = json.loads(result.stderr[start:end+1])
    if not all(math.isfinite(float(report[k])) for k in ['input_i', 'input_tp', 'input_lra']):
        raise ValueError('Nonfinite or silent source loudness')
    report['input_container'] = re.search(r'Input #0, ([^,]+),', result.stderr).group(1)
    report['measurement_stderr'] = result.stderr
    return report


def gain_db(report, target, ceiling):
    return min(target-float(report['input_i']), ceiling-float(report['input_tp']))


def pcm24(audio):
    import numpy as np
    if not np.isfinite(audio).all() or np.max(np.abs(audio)) >= 1:
        raise ValueError('Nonfinite or clipping PCM')
    values = np.rint(audio*8388608).astype('<i4').reshape(-1)
    if np.max(values) > 8388607 or np.min(values) < -8388608:
        raise ValueError('24-bit quantization overflow')
    return values.view('u1').reshape(-1, 4)[:, :3].tobytes()


def decoded(ff, path, fmt, frame_bytes, accumulator=None):
    """Hash the entire decoded stream without retaining another master copy."""
    import numpy as np
    process = subprocess.Popen([str(ff), '-v', 'error', '-nostdin', '-i', str(path),
        '-ar', str(RATE), '-ac', '2', '-f', fmt, '-'], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    h = hashlib.sha256(); count = 0; peak = 0.
    while True:
        part = process.stdout.read(1048576)
        if not part: break
        h.update(part); count += len(part)
        if accumulator is not None: accumulator.update(part)
        if fmt == 'f32le':
            samples = np.frombuffer(part, dtype='<f4')
            if not np.isfinite(samples).all(): raise ValueError('Nonfinite decoded audio')
            peak = max(peak, float(np.max(np.abs(samples))))
    error = process.stderr.read().decode('utf-8', errors='replace')
    code = process.wait(); process.stdout.close(); process.stderr.close()
    if code or count % frame_bytes: raise ValueError('Full decode failed: '+error)
    return dict(sha256=h.hexdigest(), frames=count//frame_bytes, bytes=count, sample_peak=peak)


def metadata_text(value):
    return str(value).replace('\\', '\\\\').replace('\n', ' ').replace('\r', ' ').replace('=', '\\=').replace(';', '\\;').replace('#', '\\#')


def timestamp(frames):
    seconds = frames//RATE
    return f'{seconds//3600:02}:{seconds//60%60:02}:{seconds%60:02}'


def build(q):
    runtime = read(checked(q['numeric_runtime']['path'], q['numeric_runtime']))
    checked(sys.executable, runtime['python'])
    if not runtime['files']:
        raise ValueError('Numerical runtime must be frozen')
    for item in runtime['files']:
        checked(item['path'], item)
    import numpy as np
    validate(q); ff = checked(q['ffmpeg']['path'], q['ffmpeg'])
    gap_frames = round(q['gap_seconds']*RATE); silence = bytes(gap_frames*6)
    segments = []; raw_reports = []; cursor = 0; expected = hashlib.sha256()
    with Path('master.pcm').open('xb') as master:
        for t in q['tracks']:
            source = checked(t['audio']['path'], t['audio'])
            print('TRACK', t['position'], t['song_id'], flush=True)
            report = loudness(ff, source); raw_reports.append(dict(song_id=t['song_id'], report=report))
            raw = subprocess.check_output([str(ff), '-v', 'error', '-nostdin', '-i', str(source), '-ar', str(RATE), '-ac', '2', '-f', 'f32le', '-'])
            samples = np.frombuffer(raw, dtype='<f4').reshape(-1, 2)
            if len(samples) < RATE or len(samples) > RATE*900 or not np.isfinite(samples).all() or not np.any(samples):
                raise ValueError('Invalid source extent or signal')
            gain = gain_db(report, q['target_lufs'], q['source_peak_ceiling_dbtp'])
            packed = pcm24(samples.astype('float64')*10**(gain/20))
            segments.append(dict(position=t['position'], song_id=t['song_id'], title=t['title'],
                source=t['audio'], source_golden=t['golden'], start_frame=cursor, frames=len(samples),
                neutral_decoded_float_sha256=hashlib.sha256(raw).hexdigest(),
                mastered_pcm24_sha256=hashlib.sha256(packed).hexdigest(), static_gain_db=gain,
                measured_input_lufs=float(report['input_i']), measured_input_true_peak_dbtp=float(report['input_tp'])))
            master.write(packed); expected.update(packed); cursor += len(samples)
            if t['position'] < len(q['tracks']):
                master.write(silence); expected.update(silence); cursor += gap_frames
    text = ';FFMETADATA1\ntitle='+metadata_text(q['title'])+'\nalbum='+metadata_text(q['title'])+'\n'
    for s in segments:
        text += f"[CHAPTER]\nTIMEBASE=1/{RATE}\nSTART={s['start_frame']}\nEND={s['start_frame']+s['frames']}\ntitle="+metadata_text(s['title'])+'\n'
    Path('chapters.ffmetadata').write_text(text, encoding='utf-8', newline='\n')
    subprocess.run([str(ff), '-v', 'error', '-nostdin', '-f', 's24le', '-ar', str(RATE), '-ac', '2', '-i', 'master.pcm',
        '-f', 'ffmetadata', '-i', 'chapters.ffmetadata', '-map_metadata', '1', '-map_chapters', '1',
        '-c:a', 'flac', '-compression_level', '8', 'master.flac'], check=True)
    lossless = decoded(ff, 'master.flac', 's24le', 6)
    if lossless['frames'] != cursor or lossless['sha256'] != expected.hexdigest():
        raise ValueError('Lossless master differs from exact ordered PCM and gaps')
    subprocess.run([str(ff), '-v', 'error', '-nostdin', '-i', 'master.flac', '-f', 'ffmetadata', '-i', 'chapters.ffmetadata',
        '-map_metadata', '1', '-map_chapters', '1', '-id3v2_version', '3', '-c:a', 'libmp3lame', '-b:a', '320k', 'master.mp3'], check=True)
    listening = decoded(ff, 'master.mp3', 'f32le', 8)
    if abs(listening['frames']-cursor) > round(.05*RATE) or listening['sample_peak'] >= 1:
        raise ValueError('MP3 full decode failed extent or clipping check')
    final_levels = {name:loudness(ff, 'master.'+name) for name in ['flac', 'mp3']}
    if any(float(v['input_tp']) > -1 for v in final_levels.values()):
        raise ValueError('Final master exceeds -1 dB true-peak delivery ceiling')
    cue = f'TITLE "{q["title"]}"\nFILE "{q["delivery_stem"]}.flac" WAVE\n'
    tracklist = q['title']+'\n13-track Journey album\n' if len(segments)==13 else q['title']+'\n'
    for s in segments:
        ticks = round(s['start_frame']*75/RATE)
        cue += f'  TRACK {s["position"]:02} AUDIO\n    TITLE "{s["title"].replace(chr(34), chr(39))}"\n    INDEX 01 {ticks//4500:02}:{ticks//75%60:02}:{ticks%75:02}\n'
        tracklist += f'{s["position"]:02}. {timestamp(s["start_frame"])}  {s["title"]}\n'
    tracklist += '\nDuration: '+timestamp(cursor)+'\nFull recordings; fixed gain only; '+str(q['gap_seconds'])+'-second intertrack pauses.\n'
    Path('chapters.cue').write_text(cue, encoding='utf-8', newline='\n')
    Path('tracklist.txt').write_text(tracklist, encoding='utf-8', newline='\n')
    write('level-analysis.json', dict(sources=raw_reports, final_masters=final_levels))
    write('master-check.json', dict(schema='reel.local-song-album-master-check.v1', title=q['title'],
        request=descriptor('album-request.json'), tracks=segments, frames=cursor, sample_rate=RATE, channels=2,
        duration_seconds=cursor/RATE, gap_frames=gap_frames, gap_count=len(segments)-1,
        lossless_pcm_exact=True, lossless_decode=lossless, mp3_decode=listening,
        outputs={p:descriptor(p) for p in ['master.flac','master.mp3','chapters.cue','tracklist.txt','chapters.ffmetadata','level-analysis.json']},
        mastering=dict(target_lufs=q['target_lufs'], source_peak_ceiling_dbtp=q['source_peak_ceiling_dbtp'],
            fixed_gain_only=True, dynamic_compression=False, retiming=False, crossfades=False,
            sample_format='48 kHz stereo 24-bit FLAC; 320 kbps MP3', full_sources=True),
        human_album_listening='pending', public_release=False))
    print('ALBUM VERIFIED', q['title'], cursor/RATE, flush=True)


if __name__ == '__main__':
    offline(); build(read('album-request.json'))
