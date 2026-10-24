"""Offline recording-based pulse, harmonic and section proposals for working books.

These are auditable score-editor proposals, not accepted musical transcriptions.
No generated accompaniment or expected notes are supplied to the analyzer.
"""
import argparse
import json
import os
from pathlib import Path
from local_song_rhythm import digest, validate_runtime, _peaks
from local_song_piano_arrangement import chord_path, triads

NAMES=['C','Db','D','Eb','E','F','Gb','G','Ab','A','Bb','B']


def tonal_candidates(vector):
    import numpy as np
    v=np.asarray(vector,dtype=float)
    if v.shape!=(12,) or not np.isfinite(v).all() or np.max(v)<=0:raise ValueError('No usable tonal evidence')
    # Explicit producer scale-membership heuristic, not a calibrated key model.
    profiles={'major':[3,0,1,0,2,1,0,2.5,0,1,0,1],
              'minor':[3,0,1,2,0,1,0,2.5,1,0,1,0]}
    out=[];v=v/np.linalg.norm(v)
    for quality,profile in profiles.items():
        p=np.asarray(profile,dtype=float);p/=np.linalg.norm(p)
        for root in range(12):out.append(dict(root=root,quality=quality,label=NAMES[root]+(' major' if quality=='major' else ' minor'),similarity=float(v@np.roll(p,root))))
    return sorted(out,key=lambda x:x['similarity'],reverse=True)[:4]


def section_ranges(novelty,times,duration):
    import numpy as np
    from scipy.signal import find_peaks
    values=np.asarray(novelty);t=np.asarray(times)
    if len(values)!=len(t) or not np.isfinite(values).all() or duration<=0:raise ValueError('Invalid section evidence')
    interior=(t>=8)&(t<=duration-8);threshold=float(np.median(values[interior])+np.std(values[interior])) if interior.any() else float('inf')
    spacing=max(1,round(8/max(.001,float(np.median(np.diff(t)))))) if len(t)>1 else 1
    candidates,_=find_peaks(values,height=threshold,distance=spacing)
    chosen=sorted([int(i) for i in candidates if interior[i]],key=lambda i:values[i],reverse=True)[:5]
    bounds=[0.]+sorted(float(t[i]) for i in chosen)+[float(duration)]
    return [dict(index=i+1,start=a,end=b,label='Section '+str(i+1),status='Acoustic-change proposal; formal function unreviewed') for i,(a,b) in enumerate(zip(bounds[:-1],bounds[1:]))]


def analyze(item):
    import numpy as np
    import librosa
    import soundfile as sf
    from scipy.ndimage import gaussian_filter1d
    source=Path(item['input']);identity=dict(sha256=digest(source),bytes=source.stat().st_size)
    if identity['sha256']!=item['sha256']:raise ValueError('Source identity differs')
    y,sr=librosa.load(source,sr=22050,mono=True);duration=len(y)/sr;hop=512
    if not np.isfinite(y).all() or np.sqrt(np.mean(y*y))<1e-5 or abs(duration-item['seconds'])>.002:raise ValueError('Invalid complete source signal or duration')
    D=librosa.stft(y,n_fft=2048,hop_length=hop);H,P=librosa.decompose.hpss(D,margin=2)
    harmonic=librosa.istft(H,hop_length=hop,length=len(y))
    chroma=librosa.feature.chroma_cqt(y=harmonic,sr=sr,hop_length=hop)
    times=np.arange(chroma.shape[1])*hop/sr
    onset=librosa.onset.onset_strength(S=librosa.amplitude_to_db(np.abs(P),ref=np.max),sr=sr,hop_length=hop)
    _,alternatives=_peaks(onset,sr/hop)
    tempo,frames=librosa.beat.beat_track(onset_envelope=onset,sr=sr,hop_length=hop,tightness=80)
    bpm=float(np.asarray(tempo).reshape(-1)[0]);beats=librosa.frames_to_time(frames,sr=sr,hop_length=hop)
    if not 30<=bpm<=300 or len(beats)<8:raise ValueError('Insufficient source pulse evidence')
    step=float(np.median(np.diff(beats)));grid=beats.copy()
    while grid[0]>step*.5:grid=np.r_[max(0,grid[0]-step),grid]
    while grid[-1]<duration:grid=np.r_[grid,grid[-1]+step]
    # Display bars are proposed, while every beat and alternative stays retained.
    meter=item.get('display_meter',[4,4]);group=3 if meter==[6,8] else meter[0]
    bar_edges=grid[::group];bar_edges=np.r_[0,bar_edges[(bar_edges>0)&(bar_edges<duration)],duration]
    bar_edges=np.unique(bar_edges);bars=[];vectors=[]
    for a,b in zip(bar_edges[:-1],bar_edges[1:]):
        mask=(times>=a)&(times<b)
        if b-a<.05 or not mask.any():continue
        vectors.append(np.median(chroma[:,mask],axis=1));bars.append(dict(start=float(a),end=float(b)))
    path,scores=chord_path(vectors,penalty=.2);bank=triads()
    for i,(bar,chosen) in enumerate(zip(bars,path)):
        root,quality,pcs=bank[chosen];rank=np.argsort(scores[i])[::-1];bar.update(index=i+1,symbol=NAMES[root]+('m' if quality=='minor' else ''),root=root,quality=quality,pitch_classes=pcs,template_similarity=float(scores[i,chosen]),alternative_symbol=NAMES[bank[int(rank[1])][0]]+('m' if bank[int(rank[1])][1]=='minor' else ''),status='Triad reduction proposal; inversions and extensions unreviewed')
    smooth=gaussian_filter1d(chroma,1.0*sr/hop,axis=1);smooth/=np.maximum(np.linalg.norm(smooth,axis=0),1e-8)
    lag=max(1,round(2*sr/hop));novelty=np.zeros(chroma.shape[1]);novelty[lag:-lag]=1-np.sum(smooth[:,:-2*lag]*smooth[:,2*lag:],axis=0)
    sections=section_ranges(novelty,times,duration)
    windows=[]
    for a in np.arange(0,max(.1,duration-7.99),4):
        mask=(times>=a)&(times<a+8)
        if mask.any():windows.append((float(a),np.mean(chroma[:,mask],axis=1)))
    returns=[]
    for i,(a,v) in enumerate(windows):
        for b,w in windows[:i]:
            if a-b<16:continue
            similarity=float(v@w/max(1e-10,np.linalg.norm(v)*np.linalg.norm(w)))
            if similarity>=.94:returns.append(dict(first=b,return_at=a,window_seconds=8,similarity=similarity,interpretation='Similar harmonic texture; not proof of a repeated melody'))
    returns=sorted(returns,key=lambda x:x['similarity'],reverse=True)[:8]
    feature=Path(item['features']);np.savez_compressed(feature,chroma=chroma.astype('float32'),chroma_times=times.astype('float32'),percussive_onset=onset.astype('float32'),beat_seconds=beats.astype('float32'),section_novelty=novelty.astype('float32'),sample_rate=sr,hop_length=hop)
    assert digest(source)==identity['sha256'],'Source changed during analysis'
    return dict(schema='reel.instrumental-form-harmony-proposals.v1',source=identity,seconds=duration,tempo=dict(selected_quarter_reference_bpm=bpm,beat_seconds=beats.tolist(),alternatives=alternatives[:6],status='Pulse hypothesis; half/double-time and downbeat phase need musical review'),display_meter=meter,meter_status='Producer display proposal, not a measured or approved source meter',tonal_candidates=tonal_candidates(np.mean(chroma,axis=1)),bars=bars,sections=sections,harmonic_texture_returns=returns,features=dict(sha256=digest(feature),bytes=feature.stat().st_size),source_audio_modified=False,human_musical_approval=False,method=dict(chroma='Harmonic component CQT; median per proposed display bar',harmony='24 major/minor templates and .2 transition penalty',tonal_center='Explicit weighted scale-membership heuristic; candidate ranking only',sections='Two-sided two-second harmonic change, minimum eight-second spacing; formal function unreviewed'),limitations=['Triad symbols omit chord extensions and inversions.','Instrument overlaps and separation leakage may bias harmonic estimates.','Strong tonal candidates need not be the tonic key.','Acoustic section boundaries and harmonic returns do not establish authored musical form.'])


def main():
    p=argparse.ArgumentParser();p.add_argument('--batch',required=True);p.add_argument('--runtime',required=True);args=p.parse_args();os.environ.setdefault('NUMBA_CACHE_DIR',str(Path.cwd()/'.form-numba-cache'));runtime=validate_runtime(args.runtime);batch=json.loads(Path(args.batch).read_text(encoding='utf-8'))
    if batch.get('schema')!='reel.instrumental-form-batch.v1' or batch.get('permissions')!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Explicit private offline source-form batch required')
    if not 1<=len(batch['items'])<=32:raise ValueError('Unbounded source batch')
    for item in batch['items']:
        print('ANALYZE SOURCE FORM',item['id'],flush=True);report=analyze(item);report['installed_runtime']=runtime;report['implementation_sha256']=digest(__file__);Path(item['output']).write_text(json.dumps(report,ensure_ascii=False,indent=2,allow_nan=False)+'\n',encoding='utf-8');print('RETAINED FORM/HARMONY',item['id'],flush=True)


if __name__=='__main__':main()
