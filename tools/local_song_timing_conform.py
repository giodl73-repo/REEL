"""Read-only recording-clock comparison for transferring retained lyric timing.

A timeline may cross encodings only after multiple non-silent audio windows
agree on one source offset. A failed comparison is evidence, never a fallback.
"""
import subprocess
import numpy as np
from scipy.signal import correlate
from local_song_backend import checked, offline, read, write


def compare(reference, target, rate=2000):
    if min(len(reference), len(target)) < rate * 12:
        raise ValueError('Clock comparison needs twelve seconds of audio')
    checks=[]
    for fraction in [.15,.45,.75]:
        start=int((len(reference)-rate*6)*fraction)
        window=reference[start:start+rate*6].astype(np.float64)
        window-=window.mean()
        energy=float(np.dot(window,window))
        if energy<1e-8:
            checks.append(dict(reference_seconds=start/rate,usable=False));continue
        values=target.astype(np.float64)
        dots=correlate(values,window,mode='valid',method='fft')
        sums=np.concatenate(([0.],np.cumsum(values)))
        squares=np.concatenate(([0.],np.cumsum(values*values)))
        n=len(window);local=squares[n:]-squares[:-n]-(sums[n:]-sums[:-n])**2/n
        correlations=dots/np.sqrt(np.maximum(local*energy,1e-20))
        best=int(np.argmax(correlations))
        checks.append(dict(reference_seconds=start/rate,target_seconds=best/rate,
                           offset_seconds=(best-start)/rate,correlation=float(correlations[best]),usable=True))
    valid=[c for c in checks if c['usable'] and c['correlation']>=.95]
    accepted=len(valid)==3 and abs(len(reference)-len(target))/rate<=.1 and max(c['offset_seconds'] for c in valid)-min(c['offset_seconds'] for c in valid)<=.04
    return dict(reference_seconds=len(reference)/rate,target_seconds=len(target)/rate,checks=checks,
                transferable=accepted,offset_seconds=float(np.median([c['offset_seconds'] for c in valid])) if accepted else None)


def main():
    offline();q=read('conform-request.json');checked(q['ffmpeg']['path'],q['ffmpeg']);results=[]
    for row in q['pairs']:
        arrays=[]
        for role in ['reference','target']:
            a=row[role];checked(a['path'],a)
            raw=subprocess.check_output([q['ffmpeg']['path'],'-v','error','-nostdin','-i',a['path'],'-ar','2000','-ac','1','-f','f32le','-'])
            arrays.append(np.frombuffer(raw,dtype='<f4'))
        result=dict(id=row['id'],reference=row['reference'],target=row['target'],**compare(*arrays));results.append(result)
        print(row['id'],result['transferable'],[(round(x.get('correlation',0),4),x.get('offset_seconds')) for x in result['checks']],flush=True)
    write('clock-conformance.json',dict(schema='reel.song-clock-conformance.v1',pairs=results,producer_applied=True,human_review=False))


if __name__=='__main__':main()
