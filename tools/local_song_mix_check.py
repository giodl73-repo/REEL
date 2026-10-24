"""Independently reconstruct retained arrangement layers and check delivered mixes."""
from pathlib import Path
import json
import subprocess
from local_song_backend import checked,offline,read,write,descriptor


def compare_samples(expected,actual,tolerance=2**-22):
    import numpy as np
    if expected.shape!=actual.shape or not np.isfinite(actual).all():raise ValueError('Delivered mix extent or samples differ')
    error=float(np.max(np.abs(expected-actual)))
    if error>tolerance:raise ValueError('Delivered mix differs from independently reconstructed stem sum')
    return dict(maximum_sample_error=error,tolerance=tolerance,frames=len(actual),channels=actual.shape[1],peak=float(np.max(np.abs(actual))),stem_sum_matches=True)


def check(q):
    import numpy as np
    import soundfile as sf
    rate=44100;frames=round(q['duration_seconds']*rate);common=np.zeros((frames,2),np.float32);without=np.zeros_like(common);components=[]
    for component in q['components']:
        path=checked(component['path'],component['asset'])
        with np.load(path,allow_pickle=False) as data:
            audio=data['audio'];sr=int(data['sample_rate'])
            if sr!=rate or audio.shape!=common.shape or not np.isfinite(audio).all():raise ValueError('Retained layer extent or samples differ')
            modes=component.get('modes',['with_vocals','without_vocals'])
            if modes==['without_vocals']:without+=audio*component['gain']
            elif modes==['with_vocals','without_vocals']:common+=audio*component['gain']
            else:raise ValueError('Unsupported mix-check component mode')
        components.append(dict(asset=component['asset'],gain=component['gain'],modes=modes))
    coro=np.zeros_like(common)
    if q['vocal_estimate'] is not None:
        v=q['vocal_estimate'];audio,sr=sf.read(checked(v['path'],v['asset']),dtype='float32',always_2d=True)
        if sr!=rate or audio.shape!=(round(q['source_duration_seconds']*rate),2):raise ValueError('Chorus source extent differs')
        coro[:len(audio)]=audio
    ff=checked(q['ffmpeg']['path'],q['ffmpeg']);modes=[]
    for mode,expected in [('with_vocals',common+coro),('without_vocals',common+without)]:
        source=Path(mode+'-sum.wav');filtered=Path(mode+'-expected.wav');sf.write(source,expected,rate,subtype='FLOAT')
        subprocess.run([str(ff),'-v','error','-nostdin','-i',str(source),'-af','alimiter=limit=0.95:level=false:latency=true','-c:a','pcm_f32le',str(filtered)],check=True,capture_output=True)
        expected,sr=sf.read(filtered,dtype='float32',always_2d=True);binding=q['modes'][mode];actual,ar=sf.read(checked(binding['path'],binding['asset']),dtype='float32',always_2d=True)
        if sr!=rate or ar!=rate:raise ValueError('Delivered mix sample rate differs')
        metric=compare_samples(expected,actual);metric.update(mode=mode,asset=binding['asset']);modes.append(metric)
        source.unlink();filtered.unlink()
    if q['vocal_estimate'] is None and q['modes']['with_vocals']['asset']!=q['modes']['without_vocals']['asset']:raise ValueError('Pure instrumental mode bindings must alias the same file')
    return dict(schema='reel.arrangement-stem-sum-check.v1',components=components,modes=modes,source_golden_lock=q['source_golden_lock'],limiter='Exact retained fixed limiter, latency compensated; no automatic gain',method='Independent layer read and sum, fixed-mastering reconstruction, complete delivered FLAC readback',musical_approval=False)


def main():
    offline();q=read('mix-check-request.json')
    if q['schema']!='reel.arrangement-stem-sum-request.v1' or not 1<=len(q['items'])<=14:raise ValueError('Explicit bounded arrangement mix-check batch required')
    root=Path.cwd()
    for item in q['items']:
        directory=item['directory']
        if Path(directory).name!=directory or directory in ['.','..']:raise ValueError('Invalid scoped mix-check directory')
        import os
        os.chdir(root/directory)
        try:write('mix-check.json',check(item['request']))
        finally:os.chdir(root)
        print('INDEPENDENT STEM SUM VERIFIED',directory,flush=True)


if __name__=='__main__':main()
