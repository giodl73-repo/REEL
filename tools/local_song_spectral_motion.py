"""Supplement retained pulse reports with the qualified spectral-motion method."""
import json,os
from pathlib import Path
from local_song_backend import read,write,offline,descriptor
import local_song_rhythm as qualified


def main():
    offline();q=read('motion-request.json')
    if q['schema']!='reel.spectral-motion-supplement.v1' or not 1<=len(q['items'])<=14 or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Explicit private bounded spectral-motion request required')
    runtime=qualified.validate_runtime('runtime.json');os.environ.setdefault('NUMBA_CACHE_DIR',str(Path.cwd()/'.motion-numba-cache'))
    import numpy as np
    import librosa
    import soundfile as sf
    for item in q['items']:
        path=Path(item['input']);identity=descriptor(path)
        if identity['sha256']!=item['source']['sha256'] or identity['bytes']!=item['source']['bytes']:raise ValueError('Spectral-motion source differs')
        info=sf.info(path)
        if not 20<=info.frames/info.samplerate<=900:raise ValueError('Spectral-motion duration outside declared bounds')
        method=qualified.METHOD;y,sr=librosa.load(path,sr=method['sample_rate'],mono=True)
        if not np.isfinite(y).all():raise ValueError('Nonfinite spectral-motion source')
        spectrum=np.abs(librosa.stft(y,n_fft=method['n_fft'],hop_length=method['hop_length']));harmonic,_=librosa.decompose.hpss(spectrum,margin=method['hpss_margin']);motion=qualified.spectral_motion(harmonic,np.linspace(0,sr/2,harmonic.shape[0]),sr/method['hop_length'])
        if descriptor(path)!=identity:raise ValueError('Spectral-motion source changed during analysis')
        write(item['output'],dict(schema='reel.spectral-motion-supplement.v1',source=identity,seconds=len(y)/sr,spectral_motion=motion,qualified_source=descriptor(qualified.__file__),installed_runtime=runtime,method=method,audio_modified=False,musical_approval=False));print('QUALIFIED SPECTRAL MOTION RETAINED',item['id'],flush=True)


if __name__=='__main__':main()
