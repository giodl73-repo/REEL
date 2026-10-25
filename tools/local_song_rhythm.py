"""Offline, non-destructive pulse and attack-density diagnostics.

Run private recordings through the shared Rust job contract. These are signal
estimates, not musical quality scores, instrument counts, or Golden decisions.
"""
from __future__ import annotations
import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path

METHOD = {
    'sample_rate': 22050, 'hop_length': 256, 'n_fft': 2048,
    'hpss_margin': 2.0, 'spectral_motion_seconds': 1, 'spectral_motion_bands': 24, 'onset_frequency_max_filter': 3,
    'window_seconds': 12, 'stride_seconds': 4, 'edge_exclusion_seconds': 4,
    'tempo_search_bpm': [45, 220], 'tempo_alias_folding': 'powers of two toward global dominant tempo; raw estimates retained',
    'density_min_spacing_seconds': .08,
    'harmonic_recurrence_window_seconds': 8, 'harmonic_recurrence_exclusion_seconds': 12,
    'timing_resolution_seconds': 256/22050,
    'sources': [
        'https://librosa.org/doc/0.10.2/generated/librosa.onset.onset_strength.html',
        'https://librosa.org/doc/0.10.2/generated/librosa.feature.tempogram.html',
        'https://librosa.org/doc/0.10.2/generated/librosa.segment.recurrence_matrix.html'],
    'limitations': [
        'A stable beat can coexist with a crowded arrangement.',
        'Cuban syncopation, clave and half/double-time ambiguity are not faults.',
        'Autocorrelation describes repetition; it does not establish a unique beat or meter.',
        'HPSS components and detected attack counts are estimates, not isolated instruments.',
        'No automatic musical approval, rejection, quality score or acceptance threshold.']}

def digest(path):
    h=hashlib.sha256()
    with Path(path).open('rb') as f:
        for part in iter(lambda:f.read(1024*1024),b''): h.update(part)
    return h.hexdigest()

def _correlation(x):
    import numpy as np
    from scipy.signal import correlate
    x=np.asarray(x,dtype=float);x=x-x.mean()
    ac=correlate(x,x,mode='full',method='fft')[len(x)-1:]
    energy=np.r_[0.,np.cumsum(x*x)]
    lags=np.arange(len(x));den=np.sqrt(np.maximum(energy[len(x)-lags]*(energy[-1]-energy[lags]),0))
    return np.clip(np.divide(ac,den,out=np.zeros_like(ac),where=den>1e-12),-1.,1.)

def _peaks(x,rate):
    import numpy as np
    from scipy.signal import find_peaks
    ac=_correlation(x);lo=max(1,int(rate*60/220));hi=min(len(ac)-2,int(rate*60/45))
    if hi<=lo:return ac,[]
    peaks,_=find_peaks(ac[lo:hi+1],distance=max(1,int(.035*rate)))
    ix=[int(v+lo) for v in peaks]
    if not ix:ix=[int(lo+np.argmax(ac[lo:hi+1]))]
    rows=[dict(bpm=float(60*rate/i),correlation=float(ac[i]),lag_frames=i) for i in ix]
    return ac,sorted(rows,key=lambda p:p['correlation'],reverse=True)

def _density(x,rate):
    import numpy as np
    from scipy.signal import find_peaks
    x=np.asarray(x,float)
    if not np.any(x>0):return 0.
    med=float(np.median(x));mad=float(np.median(np.abs(x-med)))
    high=float(np.percentile(x,95));threshold=max(med+mad,.15*high)
    peaks,_=find_peaks(x,height=threshold,prominence=.12*high,distance=max(1,int(.08*rate)))
    return float(len(peaks)/(len(x)/rate))

def summarize_envelope(envelope,rate,window_seconds=12,stride_seconds=4,edge_seconds=4):
    import numpy as np
    """Use raw onset autocorrelation, never a beat tracker forced onto a grid."""
    x=np.asarray(envelope,float)
    if x.ndim!=1 or not np.isfinite(x).all() or rate<=0:raise ValueError('Invalid onset envelope')
    if len(x)/rate<20:raise ValueError('At least 20 seconds required')
    if np.max(x)<=1e-10 or np.std(x)<=1e-10:return dict(status='unmeasurable',reason='No varying onset evidence',windows=[])
    edge=int(edge_seconds*rate);body=x[edge:len(x)-edge] if edge else x
    _,candidates=_peaks(body,rate);global_bpm=candidates[0]['bpm']
    win=int(window_seconds*rate);stride=int(stride_seconds*rate);windows=[]
    for start in range(edge,len(x)-edge-win+1,stride):
        chunk=x[start:start+win];ac,peaks=_peaks(chunk,rate)
        if not peaks:continue
        raw=peaks[0]['bpm'];folded=raw*2**round(np.log2(global_bpm/raw))
        period=60*rate/global_bpm;supports={}
        for mult in [.5,1,2,4,8]:
            lag=int(round(period*mult))
            if lag<.7*len(ac):
                radius=max(1,int(.035*lag));sl=ac[max(1,lag-radius):min(len(ac),lag+radius+1)]
                supports[str(mult)]=float(max(0.,np.max(sl)))
        long_lo=int(1.5*rate);long_hi=min(len(ac)-1,int(.6*len(ac)));long_lag=long_lo+int(np.argmax(ac[long_lo:long_hi+1]))
        windows.append(dict(long_cycle_repetition=float(max(0.,ac[long_lag])),long_cycle_seconds=long_lag/rate,start_seconds=start/rate,end_seconds=(start+win)/rate,
            raw_dominant_bpm=raw,octave_folded_bpm=folded,
            dominant_correlation=peaks[0]['correlation'],
            short_cycle_repetition=max((v for k,v in supports.items() if k in ['0.5','1','2']),default=0.),
            cycle_correlations=supports,detected_attacks_per_second=_density(chunk,rate)))
    if not windows:return dict(status='unmeasurable',reason='Insufficient windows',windows=[])
    tempos=np.array([v['octave_folded_bpm'] for v in windows]);strength=np.array([v['short_cycle_repetition'] for v in windows]);density=np.array([v['detected_attacks_per_second'] for v in windows])
    return dict(status='measured',dominant_bpm=global_bpm,tempo_candidates=candidates[:8],
        folded_local_tempo_median=float(np.median(tempos)),
        folded_local_tempo_iqr_percent=float(100*(np.percentile(tempos,75)-np.percentile(tempos,25))/np.median(tempos)),
        folded_local_tempo_10_90_percent=float(100*(np.percentile(tempos,90)-np.percentile(tempos,10))/np.median(tempos)),
        short_cycle_repetition_median=float(np.median(strength)),short_cycle_repetition_p10=float(np.percentile(strength,10)),
        long_cycle_repetition_median=float(np.median([v['long_cycle_repetition'] for v in windows])),
        detected_attacks_per_second=float(np.mean(density)),
        attack_density_cv_percent=float(100*np.std(density)/np.mean(density)) if np.mean(density)>0 else None,
        windows=windows)

def harmonic_recurrence(chroma,frame_rate):
    import numpy as np
    """Eight-second chroma trajectories, compared at least twelve seconds apart.

    This is a harmonic-pattern proxy; it does not recover a melody or certify
    compositional form. Constant tonal content can also yield high recurrence.
    """
    x=np.asarray(chroma,float);seconds=int(x.shape[1]/frame_rate)
    if seconds<24:return dict(status='unmeasurable')
    one=np.stack([x[:,int(t*frame_rate):int((t+1)*frame_rate)].mean(axis=1) for t in range(seconds)],axis=1)
    starts=list(range(4,seconds-4-8+1,2));vectors=np.array([one[:,t:t+8].reshape(-1) for t in starts]);norm=np.linalg.norm(vectors,axis=1)
    if not len(vectors) or not np.any(norm>1e-12):return dict(status='unmeasurable')
    unit=np.divide(vectors,norm[:,None],out=np.zeros_like(vectors),where=norm[:,None]>1e-12);sim=np.clip(unit@unit.T,-1,1);far=np.abs(np.subtract.outer(starts,starts))>=12;sim[~far]=-1
    best=np.max(sim,axis=1);valid=best>=0
    if not np.any(valid):return dict(status='unmeasurable',reason='No sufficiently separated comparison windows')
    return dict(status='measured',harmonic_pattern_recurrence_median=float(np.median(best[valid])),
        windows=[dict(start_seconds=t,best_nonadjacent_similarity=float(v)) for t,v in zip(starts,best) if v>=0],
        interpretation='Chroma trajectory recurrence; not direct melody recognition')

def spectral_motion(spectrum,frequencies,frame_rate):
    """Register spread and timbral movement; neither is a melody-quality score."""
    import numpy as np
    S=np.asarray(spectrum,float);f=np.asarray(frequencies,float);seconds=int(S.shape[1]/frame_rate)
    if seconds<20 or np.max(S)<=1e-10:return dict(status='unmeasurable')
    bands=np.geomspace(60,min(8000.,f[-1]),25);features=[]
    for lo,hi in zip(bands[:-1],bands[1:]):
        mask=(f>=lo)&(f<hi);features.append(np.sqrt(np.sum(S[mask]**2,axis=0)))
    B=np.asarray(features);unitseconds=np.stack([B[:,int(t*frame_rate):int((t+1)*frame_rate)].mean(axis=1) for t in range(4,seconds-4)],axis=0);norm=np.linalg.norm(unitseconds,axis=1);U=np.divide(unitseconds,norm[:,None],out=np.zeros_like(unitseconds),where=norm[:,None]>1e-12);novelty=np.clip(1-np.sum(U[1:]*U[:-1],axis=1),0,1)
    energy=np.sum(S,axis=0);centroid=np.divide(np.sum(S*f[:,None],axis=0),energy,out=np.zeros_like(energy),where=energy>1e-12);register=np.array([np.median(centroid[int(t*frame_rate):int((t+1)*frame_rate)]) for t in range(4,seconds-4)]);median=float(np.median(register));spread=float(100*(np.percentile(register,75)-np.percentile(register,25))/median) if median>0 else None
    return dict(status='measured',harmonic_centroid_median_hz=median,harmonic_centroid_iqr_percent=spread,texture_change_mean=float(np.mean(novelty)),texture_change_p90=float(np.percentile(novelty,90)),one_second_windows=[dict(start_seconds=t+4,centroid_hz=float(v),texture_change_from_previous=float(novelty[t-1]) if t else None) for t,v in enumerate(register)],interpretation='Spectral-register and energy-distribution changes; not identified instruments or melody transcription')

def analyze(path,expected_sha256=None):
    import numpy as np
    import librosa
    import soundfile as sf
    source=Path(path)
    identity=dict(sha256=digest(source),bytes=source.stat().st_size)
    if expected_sha256 and identity['sha256']!=expected_sha256:raise ValueError('Source hash mismatch')
    info=sf.info(source)
    if info.frames/info.samplerate>900:raise ValueError('Input exceeds 15-minute bound')
    y,sr=librosa.load(source,sr=METHOD['sample_rate'],mono=True)
    if not np.isfinite(y).all():raise ValueError('Nonfinite audio')
    if len(y)/sr>900:raise ValueError('Input exceeds 15-minute bound')
    hop=METHOD['hop_length'];D=np.abs(librosa.stft(y,n_fft=METHOD['n_fft'],hop_length=hop))
    harmonic,percussive=librosa.decompose.hpss(D,margin=METHOD['hpss_margin'])
    result={}
    for name,S in [('percussive',percussive),('harmonic',harmonic)]:
        if np.max(S)<=1e-10:oe=np.zeros(S.shape[1])
        else:oe=librosa.onset.onset_strength(S=librosa.amplitude_to_db(S,ref=np.max),sr=sr,hop_length=hop,max_size=3,center=False)
        result[name]=dict(summary=summarize_envelope(oe,sr/hop),envelope=np.round(oe,6).tolist())
    chroma=librosa.feature.chroma_stft(S=harmonic**2,sr=sr,hop_length=hop,n_fft=METHOD['n_fft'])
    recurrence=harmonic_recurrence(chroma,sr/hop)
    motion=spectral_motion(harmonic,np.linspace(0,sr/2,harmonic.shape[0]),sr/hop)
    assert digest(source)==identity['sha256'],'Source changed during analysis'
    return dict(schema='reel.local-song-rhythm.v1',source=identity,seconds=len(y)/sr,method=METHOD,
        implementation=dict(sha256=digest(__file__),versions={p:importlib.metadata.version(p) for p in ['numpy','scipy','librosa','soundfile']}),
        estimates=result,harmonic_recurrence=recurrence,spectral_motion=motion,audio_modified=False,musical_approval=False)

def validate_runtime(path):
    import sys
    data=json.loads(Path(path).read_text(encoding='utf-8'))
    if data.get('schema')!='reel.local-song-rhythm-runtime.v1':raise ValueError('Unknown rhythm runtime')
    items=[dict(data['python'],path=sys.executable)]+data['files']
    for item in items:
        p=Path(item['path'])
        if p.stat().st_size!=item['bytes'] or digest(p)!=item['sha256']:raise ValueError('Installed numeric runtime changed: '+str(p))
    return dict(sha256=digest(path),bytes=Path(path).stat().st_size)

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--input');p.add_argument('--expected-sha256');p.add_argument('--output');p.add_argument('--batch');p.add_argument('--runtime');args=p.parse_args()
    runtime=validate_runtime(args.runtime) if args.runtime else None
    os.environ.setdefault('NUMBA_CACHE_DIR',str(Path.cwd()/'.rhythm-numba-cache'))
    if args.batch:
        items=json.loads(Path(args.batch).read_text(encoding='utf-8'))['items']
        if not 1<=len(items)<=128:raise ValueError('Batch must have 1 to 128 named recordings')
    else:
        if not all([args.input,args.expected_sha256,args.output]):p.error('Supply --batch, or --input/--expected-sha256/--output')
        items=[dict(input=args.input,sha256=args.expected_sha256,output=args.output)]
    for item in items:
        print('ANALYZE',item.get('id',item['input']),flush=True)
        report=analyze(item['input'],item['sha256']);report['installed_runtime']=runtime
        with Path(item['output']).open('x',encoding='utf-8') as f:json.dump(report,f,ensure_ascii=False,indent=2,allow_nan=False);f.write('\n')
        print('RETAINED',item['output'],flush=True)
if __name__=='__main__':main()
