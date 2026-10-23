"""Bounded spectral inspection and local, non-generative parametric EQ trials."""
import argparse
import importlib.metadata
from pathlib import Path
import subprocess
import numpy as np
import soundfile as sf
from scipy import signal
from local_song_backend import checked, descriptor, read, write


def peaking_section(rate, frequency, q, gain_db):
    if not 100 <= frequency < min(18000, rate*.45) or not .5 <= q <= 40 or not -24 <= gain_db <= 0:
        raise ValueError('Invalid bounded parametric cut')
    # Forward/backward filtering squares the magnitude response; design half
    # the requested dB cut per pass. Phase remains unchanged away from edges.
    a=10**(gain_db/80);omega=2*np.pi*frequency/rate;alpha=np.sin(omega)/(2*q)
    b=np.array([1+alpha*a,-2*np.cos(omega),1-alpha*a]);den=np.array([1+alpha/a,-2*np.cos(omega),1-alpha/a])
    return np.r_[b/den[0],den/den[0]]


def repair(original, rate, start, end, bands, fade=.25):
    if not 0 <= start < end <= len(original)/rate or end-start>10 or not 0<fade<=(end-start)/2:
        raise ValueError('Invalid bounded EQ mask')
    if not 1<=len(bands)<=4:raise ValueError('Expected one to four explicit EQ bands')
    a,b=round(start*rate),round(end*rate);lo=max(0,a-round(rate));hi=min(len(original),b+round(rate))
    sos=np.stack([peaking_section(rate,x['hz'],x['q'],x['gain_db']) for x in bands])
    filtered=signal.sosfiltfilt(sos,original[lo:hi].astype(np.float64),axis=0)[a-lo:b-lo]
    gain=np.ones(b-a,dtype=np.float64);n=round(fade*rate);ramp=np.sin(np.linspace(0,np.pi/2,n))**2
    gain[:n]=ramp;gain[-n:]=ramp[::-1]
    result=original.copy();result[a:b]=(original[a:b]+gain[:,None]*(filtered-original[a:b])).astype(np.float32)
    if not np.array_equal(result[:a],original[:a]) or not np.array_equal(result[b:],original[b:]):
        raise ValueError('Protected samples changed')
    return result


def spectrum(samples, rate, start, end):
    x=samples[round(start*rate):round(end*rate)].mean(axis=1).astype(np.float64)
    _,_,z=signal.stft(x,fs=rate,nperseg=8192,noverlap=6144,boundary=None)
    power=np.mean(abs(z)**2,axis=1);freq=np.fft.rfftfreq(8192,1/rate)
    peaks,_=signal.find_peaks(10*np.log10(power+1e-20),distance=3,prominence=3)
    peaks=[i for i in peaks if 500<=freq[i]<=6500]
    peaks=sorted(peaks,key=lambda i:power[i],reverse=True)[:12]
    rows=[]
    for i in peaks:
        y=np.log(power[i-1:i+2]+1e-25);offset=.5*(y[0]-y[2])/(y[0]-2*y[1]+y[2])
        rows.append(dict(hz=round(float((i+offset)*rate/8192),2),db_relative_spectral_max=round(float(10*np.log10(power[i]/power.max())),2)))
    return dict(start=start,end=end,peaks=rows)


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('operation',choices=['analyze','process']);args=parser.parse_args()
    q=read('request.json');runtime=read('runtime.json')
    if q.get('schema')!='reel.local-frequency-repair.v1' or q.get('permissions')!=dict(private_only=True,external_upload=False,identifiable_imitation=False,public_release=False):raise ValueError('Unsupported private frequency-repair request')
    checked('source.wav',q['source']);checked(runtime['python']['path'],runtime['python'])
    for package,version in q['packages'].items():
        if importlib.metadata.version(package)!=version:raise ValueError('Installed analysis package changed: '+package)
    x,rate=sf.read('source.wav',dtype='float32',always_2d=True)
    if not np.isfinite(x).all():raise ValueError('Nonfinite source')
    windows=q['windows'];report=dict(schema='reel.frequency-inspection.v1',source=descriptor('source.wav'),request=descriptor('request.json'),rate=rate,duration_seconds=len(x)/rate,windows=[spectrum(x,rate,*window) for window in windows],limits='Mixed-recording spectral peaks can be fundamentals or harmonics of several instruments; this does not establish flute intonation or isolate a flute stem.',human_listening='pending')
    if args.operation=='process':
        ffmpeg=runtime['ffmpeg'];checked(ffmpeg['path'],ffmpeg)
        results=[]
        for trial in q['trials']:
            identity=trial['id']
            if not identity.isalnum():raise ValueError('Unsafe trial id')
            y=repair(x,rate,*q['mask_seconds'],trial['bands'],q['fade_seconds'])
            if not np.isfinite(y).all() or abs(y).max()>1.001:raise ValueError('Invalid filtered signal')
            wav=identity+'.wav';mp3=identity+'.mp3';sf.write(wav,y,rate,subtype='FLOAT');retained,sr=sf.read(wav,dtype='float32',always_2d=True)
            if sr!=rate or not np.array_equal(y,retained):raise ValueError('Lossless write/read differs')
            subprocess.run([ffmpeg['path'],'-v','error','-i',wav,'-c:a','libmp3lame','-b:a','256k',mp3],check=True)
            decoded=np.frombuffer(subprocess.check_output([ffmpeg['path'],'-v','error','-i',mp3,'-ac','1','-ar','16000','-f','f32le','-']),dtype=np.float32)
            if not np.isfinite(decoded).all() or abs(len(decoded)/16000-len(x)/rate)>.05:raise ValueError('Preview decode failed')
            results.append(dict(id=identity,bands=trial['bands'],wav=descriptor(wav),mp3=descriptor(mp3),complete_decode=True,unmasked_lossless_samples_exact=True,generated_audio=False,tempo_changed=False,pitch_retuned=False,mask_seconds=q['mask_seconds'],after=[spectrum(y,rate,*window) for window in windows]))
        report['trials']=results
    write('report.json',report)


if __name__=='__main__':main()
