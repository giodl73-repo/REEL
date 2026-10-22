"""Offline, source-bound melody hypotheses for REEL's existing music contracts.

The native Rust exporter owns MIDI/MusicXML/SVG rendering and round-trip checks.
This adapter never rewrites a source, supplies lyrics to a pitch estimator, or
claims that a separated voice, inferred note, meter or key has been reviewed.
"""
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import subprocess

from local_song_backend import checked, descriptor, offline, read, write

ROLES = ['music-reconstruction-engineer', 'score-arrangement-director',
         'sound-designer', 'editor', 'rights-provenance-steward']


def note_runs(midi, valid, probabilities, hop_seconds, minimum_seconds=.12):
    """Retain nonoverlapping stable pitch runs; unknown frames stay unknown."""
    import numpy as np
    from scipy.ndimage import median_filter
    values = np.where(valid, np.rint(np.nan_to_num(midi)), -1).astype(int)
    smooth = median_filter(values, size=7, mode='nearest')
    smooth[~valid] = -1
    changes = np.r_[0, np.flatnonzero(np.diff(smooth))+1, len(smooth)]
    result = []
    for a,b in zip(changes[:-1], changes[1:]):
        if smooth[a] < 0 or (b-a)*hop_seconds < minimum_seconds:
            continue
        result.append(dict(start=round(a*hop_seconds,6),end=round(b*hop_seconds,6),
                           midi_note=int(smooth[a]),confidence=float(np.median(probabilities[a:b])),
                           median_cents=float(np.median((midi[a:b]-smooth[a])*100))))
    return result


def native(command, *args):
    result = subprocess.run([str(Path('reel.exe').resolve()), command, *map(str,args),
                             '--output','json'],capture_output=True,text=True,encoding='utf-8')
    if result.returncode:
        raise RuntimeError(result.stderr)
    return json.loads(result.stdout)


def main():
    offline()
    request, runtime = read('transcription.json'), read('transcription-runtime.json')
    if request['schema'] != 'reel.local-song-transcription.v1':
        raise ValueError('Unsupported transcription request')
    if request['permissions'] != dict(private_only=True,external_upload=False,public_release=False):
        raise ValueError('Only private offline draft transcription is supported')
    for path,key in [('source.wav','recording'),('vocals.wav','vocal_estimate'),('lyrics.txt','lyrics')]:
        checked(path,request[key])
    for name,version in runtime['packages'].items():
        if importlib.metadata.version(name) != version:
            raise ValueError('Installed dependency version changed: '+name)
    for item in runtime['files']:
        checked(item['path'],item)
    checked('reel.exe',request['reel_runtime'])
    checked(runtime['ffmpeg']['path'],runtime['ffmpeg'])
    import librosa
    import numpy as np
    sr, hop, frame = 16000, 160, 1024
    def decode(name):
        return np.frombuffer(subprocess.check_output([runtime['ffmpeg']['path'],'-v','error','-i',name,
            '-ac','1','-ar',str(sr),'-f','f32le','-']),dtype='<f4').copy()
    mix, vocal = decode('source.wav'),decode('vocals.wav')
    if abs(len(mix)-len(vocal))>sr//5 or not np.isfinite(mix).all() or not np.isfinite(vocal).all():
        raise ValueError('Invalid source/stem signal or timebase')
    vocal=np.pad(vocal,(0,max(0,len(mix)-len(vocal))))[:len(mix)]
    Path('source.pcm').write_bytes(mix.astype('<f4').tobytes())
    parameters=dict(sample_rate=sr,hop=hop,frame=frame,fmin=65.4064,fmax=1046.502,
                    voiced_probability_threshold=.65,minimum_note_seconds=.12,
                    original_mix_agreement_cents=75,canonical_prompt=False)
    write('parameters.json',parameters)
    print('Estimate full-source vocal F0; no lyric or score prompt',flush=True)
    f0,voiced,prob=librosa.pyin(vocal,fmin=parameters['fmin'],fmax=parameters['fmax'],
                               sr=sr,frame_length=frame,hop_length=hop,fill_na=np.nan)
    # A different estimator on the original mix exposes octave/bleed conflicts.
    mix_f0=librosa.yin(mix,fmin=parameters['fmin'],fmax=parameters['fmax'],sr=sr,
                       frame_length=frame,hop_length=hop)
    rms=librosa.feature.rms(y=vocal,frame_length=frame,hop_length=hop)[0]
    valid=voiced & (prob>=parameters['voiced_probability_threshold']) & (rms>.003)
    midi=librosa.hz_to_midi(f0)
    events=note_runs(midi,valid,prob,hop/sr)
    if not events:
        raise ValueError('No stable pitch hypotheses recovered')
    for event in events:
        a,b=round(event['start']*sr/hop),round(event['end']*sr/hop)
        distance=np.abs(1200*np.log2(mix_f0[a:b]/librosa.midi_to_hz(event['midi_note'])))
        event['mix_agreement_fraction']=float(np.mean(distance<=parameters['original_mix_agreement_cents']))
        event['review_required']=True
    np.savez_compressed('pitch-frames.npz',f0=f0,voiced=voiced,probability=prob,
                        mix_f0=mix_f0,valid=valid,sample_rate=sr,hop=hop)
    tempo,beats=librosa.beat.beat_track(y=mix,sr=sr,hop_length=hop)
    bpm=float(np.asarray(tempo).reshape(-1)[0])
    if not 20<=bpm<=300:
        raise ValueError('Unusable tempo hypothesis')
    duration=len(mix)/sr
    for e in events:e['end']=min(e['end'],duration)
    events=[e for e in events if e['end']>e['start']]
    summary=dict(schema='reel.local-song-pitch-evidence.v1',source=request['recording'],
                 vocal_estimate=request['vocal_estimate'],full_duration_seconds=duration,
                 tempo_bpm=bpm,beats_seconds=(beats*hop/sr).tolist(),events=events,
                 source_modified=False,lyrics_used_for_pitch=False,human_review='pending',
                 limitations=['Estimated vocal stem may contain accompaniment bleed.',
                 'F0 estimates can contain octave errors, missed notes and merged repeated pitches.',
                 'Agreement with original mix is diagnostic, not independent human confirmation.',
                 'No lyric underlay, key, harmony, accompaniment or final meter is selected.'])
    write('pitch-evidence.json',summary)
    authority=dict(namespace='private-songbook-draft',artifact_id=request['candidate_id'],
                   content_sha256=request['authority']['sha256'],status='draft-not-reviewed',
                   required_roles=ROLES,decision_refs=[])
    timebase=dict(pulses_per_quarter=960,rounding='half-away-from-zero')
    pcm=descriptor('source.pcm')
    source=dict(schema='reel.music-source.v0.1',source_id=request['candidate_id'],
        media=dict(path='source.pcm',sha256=pcm['sha256'],format='raw-pcm-f32le',
                   timebase=dict(sample_rate_hz=sr,channels=1,samples_per_channel=len(mix)),
                   decoded_pcm_sha256=pcm['sha256']),musical_timebase=timebase,authority=authority,
        egress=dict(private=True,network_policy='denied',third_party_upload=False))
    write('source.json',source)
    srpt=native('music-source-validate','source.json')
    binding=dict(manifest='source.json',manifest_sha256=srpt['manifest_sha256'],
                 contract_sha256=srpt['contract_sha256'],decoded_pcm_sha256=pcm['sha256'])
    aid=request['candidate_id']+'-pitch-evidence'
    def observation(oid,start,end,value,confidence):
        return dict(id=oid,analyzer_id='offline-f0',source=dict(start=start,end=end),
                    confidence_millionths=round(confidence*1e6),uncertainty='Unreviewed machine hypothesis',value=value)
    obs=[observation('tempo',0,len(mix),dict(kind='tempo',milli_bpm=round(bpm*1000)),.4)]
    for i,e in enumerate(events):
        obs.append(observation(f'pitch-{i:04}',round(e['start']*sr),min(len(mix),round(e['end']*sr)),
                   dict(kind='pitch',midi_note=e['midi_note'],cents=round(e['median_cents'])),e['confidence']))
    analysis=dict(schema='reel.music-analysis.v0.1',analysis_id=aid,source=binding,
        analyzers=[dict(id='offline-f0',adapter='librosa-pyin-and-original-mix-yin',version=librosa.__version__,
                        model_revision='no-pretrained-model',parameters_sha256=descriptor('parameters.json')['sha256'],
                        license='librosa ISC; installed dependency licenses retained separately',network_policy='denied')],
        stems=[],observations=obs,limitations=summary['limitations'],
        review=dict(status='not-reviewed',required_roles=ROLES,decision_refs=[]))
    write('analysis.json',analysis)
    arpt=native('music-analysis-validate','analysis.json')
    def provenance(oid,rationale):
        return dict(state='inferred',evidence_refs=[dict(analysis_id=aid,observation_id=oid)],
                    rationale=rationale,correction_ref=None)
    usq=round(60e6/bpm)
    ticks=lambda seconds:round(seconds*1e6/usq*960)
    notes=[]
    for i,e in enumerate(events):
        start=ticks(e['start']);end=ticks(e['end'])
        notes.append(dict(id=f'note-{i:04}',voice=1,start_tick=start,duration_ticks=max(1,end-start),
                          midi_note=e['midi_note'],velocity=72,
                          provenance=provenance(f'pitch-{i:04}','Unreviewed stable F0 run; original-mix agreement retained in pitch-evidence.json')))
    model=dict(schema='reel.music-model.v0.1',model_id=request['candidate_id'],source=binding,
        analyses=[dict(manifest='analysis.json',manifest_sha256=arpt['manifest_sha256'],
                       contract_sha256=arpt['contract_sha256'],analysis_id=aid)],authority=authority,
        musical_timebase=timebase,duration_ticks=ticks(duration),
        tempo_map=[dict(tick=0,microseconds_per_quarter=usq,provenance=provenance('tempo','Global beat-tracker tempo hypothesis; local tempo and beat phase need review'))],
        meter_map=[dict(tick=0,numerator=4,denominator=4,provenance=provenance('tempo','Provisional 4/4 display grid only; meter was not measured or approved'))],
        form=[dict(id='full-recording',label='Full recording; phrase form unresolved',
                   range=dict(start=0,end=ticks(duration)),
                   provenance=provenance('tempo','Undivided complete source extent, not a musical form analysis'))],
        parts=[dict(id='melody-hypothesis',role='melody',name='Unreviewed melody hypothesis',notes=notes)],
        harmony=[],rhythm_cells=[],hooks=[],lyric_layers=[dict(id='canonical-lyrics',kind='canonical',language='es',
            path='lyrics.txt',sha256=request['lyrics']['sha256'],authority=authority)],
        lead_sheet=dict(title=request['title']+' — UNREVIEWED MELODY DRAFT',melody_part_id='melody-hypothesis',
                        lyric_layer_id=None,underlay=[]),expressive_timing=[],
        unknowns=summary['limitations']+['Canonical words are not aligned under notes; performed repeats remain unresolved.',
            '4/4 is a display placeholder, not an observed time signature.'],
        review=dict(status='not-reviewed',required_roles=ROLES,decision_refs=[]))
    write('model.json',model)
    mrpt=native('music-model-validate','model.json')
    native('music-score-export-plan','model.json','--output-path','score-plan.json')
    native('music-score-export-render','score-plan.json','model.json','--output-dir','rendered')
    check=native('music-score-export-check','rendered/receipt.json','score-plan.json','model.json','rendered')
    write('native-checks.json',dict(source=srpt,analysis=arpt,model=mrpt,score_export=check))
    for filename in ['score.mid','score.musicxml','lead-sheet.svg','rehearsal-guide.wav','receipt.json']:
        os.link(Path('rendered')/filename,filename)
    print('NATIVE SCORE DRAFT VERIFIED',len(notes),'note hypotheses',flush=True)


if __name__=='__main__':main()
