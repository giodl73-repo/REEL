"""Offline piano accompaniment proposals using REEL's native music contracts.

Beat/chord estimates and producer voicing are explicit hypotheses. Rendering
uses the installed sample-based score engine, followed by a note-clock audit.
No source singing is regenerated, retimed or selected by this adapter.
"""
import copy
import json
import os
from pathlib import Path
import subprocess
from local_song_backend import checked, descriptor, offline, read, write
from local_song_transcribe import ROLES


def triads():
    return [(root,quality,[(root+x)%12 for x in intervals])
            for root in range(12) for quality,intervals in [('major',(0,4,7)),('minor',(0,3,7))]]


def chord_path(chromas, penalty=.28):
    import numpy as np
    bank=triads();templates=np.zeros((24,12))
    for i,(_,_,pcs) in enumerate(bank):templates[i,pcs]=[1,.85,.7]
    templates/=np.linalg.norm(templates,axis=1)[:,None]
    vectors=np.asarray(chromas);vectors=vectors/np.maximum(1e-8,np.linalg.norm(vectors,axis=1))[:,None]
    if vectors.ndim!=2 or vectors.shape[1]!=12 or not len(vectors) or not np.isfinite(vectors).all():raise ValueError('Invalid chroma evidence')
    scores=vectors@templates.T;best=scores[0].copy();parents=[]
    for row in scores[1:]:
        transitions=best[:,None]-penalty*(1-np.eye(24))
        parent=np.argmax(transitions,axis=0);parents.append(parent)
        best=row+transitions[parent,np.arange(24)]
    path=[int(np.argmax(best))]
    for parent in reversed(parents):path.append(int(parent[path[-1]]))
    return list(reversed(path)),scores


def voicing(root,pcs,previous=None):
    """Close mid-register triad with minimal motion from the prior voicing."""
    import itertools
    options=[]
    for values in itertools.product(*[[n for n in range(48,73) if n%12==pc] for pc in pcs]):
        notes=sorted(values)
        if len(set(notes))!=3 or notes[-1]-notes[0]>12:continue
        cost=sum(abs(n-p) for n,p in zip(notes,previous or [55,60,64]))
        options.append((cost,notes))
    if not options:raise ValueError('No playable close piano voicing')
    return min(options)[1]


def audit_notes(expected,actual,tolerance=.035):
    """Reject dropped/added notes and pitch/timing drift by the sound renderer."""
    a=sorted(expected,key=lambda n:(n['pitch'],n['start'],n['end']))
    b=sorted(actual,key=lambda n:(n['pitch'],n['start'],n['end']))
    if len(a)!=len(b):raise ValueError(f'Sound renderer changed note count: {len(a)} to {len(b)}')
    if any(x['pitch']!=y['pitch'] for x,y in zip(a,b)):raise ValueError('Sound renderer changed piano pitches')
    onset=float(max((abs(x['start']-y['start']) for x,y in zip(a,b)),default=0))
    release=float(max((abs(x['end']-y['end']) for x,y in zip(a,b)),default=0))
    result=dict(notes=len(a),maximum_onset_error_seconds=onset,maximum_release_error_seconds=release,tolerance_seconds=tolerance,passed=onset<=tolerance and release<=tolerance)
    if not result['passed']:raise ValueError('Sound renderer altered source clocks: '+json.dumps(result))
    return result


def midi_events(path):
    """Read tempo events on every track, including MuseScore's part tracks."""
    import mido
    midi=mido.MidiFile(path);tempo=500000;seconds=0.;active={};notes=[]
    for message in mido.merge_tracks(midi.tracks):
        seconds+=mido.tick2second(message.time,midi.ticks_per_beat,tempo)
        if message.type=='set_tempo':tempo=message.tempo
        elif message.type=='note_on' and message.velocity:
            key=(message.channel,message.note)
            if key in active:raise ValueError('Overlapping identical MIDI key')
            active[key]=seconds
        elif message.type=='note_off' or message.type=='note_on' and not message.velocity:
            key=(message.channel,message.note)
            if key not in active:raise ValueError('Unmatched MIDI note release')
            notes.append(dict(pitch=message.note,start=active.pop(key),end=seconds))
    if active:raise ValueError('Unreleased MIDI notes')
    return notes


def native(request,command,*args):
    executable=checked(request['native_runtime']['path'],request['native_runtime'])
    result=subprocess.run([str(executable),command,*map(str,args),'--output','json'],capture_output=True,text=True,encoding='utf-8')
    if result.returncode:raise RuntimeError(result.stderr)
    return json.loads(result.stdout)


def analyze(request):
    import numpy as np
    import librosa
    import soundfile as sf
    from scipy.signal import resample_poly
    for filename,key in [('original.wav','recording'),('vocals.wav','vocal_estimate'),('parent-model.json','parent_model')]:checked(filename,request[key])
    mix,rate=sf.read('original.wav',dtype='float32',always_2d=True)
    vocals,vrate=sf.read('vocals.wav',dtype='float32',always_2d=True)
    if abs(len(mix)/rate-len(vocals)/vrate)>1/max(rate,vrate):raise ValueError('Vocal/source duration differs')
    sr=22050;mono=librosa.resample(mix.mean(axis=1),orig_sr=rate,target_sr=sr)
    harmonic=librosa.effects.harmonic(mono,margin=4)
    onset=librosa.onset.onset_strength(y=mono,sr=sr,hop_length=256)
    tempo,frames=librosa.beat.beat_track(onset_envelope=onset,sr=sr,hop_length=256,start_bpm=86,tightness=80)
    beats=librosa.frames_to_time(frames,sr=sr,hop_length=256)
    if len(beats)<12:raise ValueError('Insufficient independently estimated beat support')
    bpm=float(np.asarray(tempo).reshape(-1)[0])
    # A slow waltz direction is explicit; retain the faster tracker result as evidence.
    folded=bpm>120
    if folded:beats=beats[::2];bpm/=2
    if not 45<=bpm<=120:raise ValueError('Estimated piano pulse outside the declared slow-waltz direction')
    step=float(np.median(np.diff(beats)));duration=len(mix)/rate
    # Choose a three-beat phase from low-register onset support, not the lyrics.
    bass=librosa.stft(mono,n_fft=2048,hop_length=256)
    freqs=librosa.fft_frequencies(sr=sr,n_fft=2048)
    low=np.abs(bass[(freqs>=45)&(freqs<=230)]).mean(axis=0)
    low_onset=np.maximum(0,np.diff(low,prepend=low[0]))
    strengths=np.interp(beats,np.arange(len(low_onset))*256/sr,low_onset)
    phase=max(range(3),key=lambda p:float(np.mean(strengths[p::3])))
    start=phase
    grid=beats.copy();labels=np.arange(len(grid))-phase
    while grid[0]>step*.45:grid=np.r_[grid[0]-step,grid];labels=np.r_[labels[0]-1,labels]
    while grid[-1]<duration:grid=np.r_[grid,grid[-1]+step];labels=np.r_[labels,labels[-1]+1]
    chroma=librosa.feature.chroma_cqt(y=harmonic,sr=sr,hop_length=512)
    chroma_times=np.arange(chroma.shape[1])*512/sr
    starts=[i for i,label in enumerate(labels) if label%3==0 and 0<=grid[i]<duration]
    if starts and grid[starts[0]]>.2:starts=[0]+starts
    bars=[];vectors=[]
    for j,index in enumerate(starts):
        a=max(0,float(grid[index]));end_index=starts[j+1] if j+1<len(starts) else len(grid)-1;b=min(duration,float(grid[end_index]))
        if b-a<.12:continue
        selected=(chroma_times>=a)&(chroma_times<b)
        vector=np.median(chroma[:,selected],axis=1) if selected.any() else np.zeros(12)
        vectors.append(vector);bars.append(dict(start=a,end=b,beat_indices=list(range(index,min(index+3,len(grid)-1)))))
    path,scores=chord_path(vectors);bank=triads();names=['C','C#','D','Eb','E','F','F#','G','Ab','A','Bb','B']
    parent=read('parent-model.json');source=read('source.json');source_rate=source['media']['timebase']['sample_rate_hz'];source_frames=source['media']['timebase']['samples_per_channel']
    parameters=dict(chroma='librosa chroma_cqt median per proposed three-beat bar',beat_tracker=dict(raw_bpm=bpm*(2 if folded else 1),folded=folded,selected_bpm=bpm,phase=phase),direction='Producer-proposed gentle piano waltz; three-four is a direction, not a verified source meter',chord_transition_penalty=.28,triad_vocabulary='12 major and 12 minor triads',source_audio_modified=False)
    write('parameters.json',parameters)
    aid=request['arrangement_id']+'-harmony-analysis';analyzer='offline-harmony';obs=[]
    def observe(oid,a,b,value,confidence=.4):
        obs.append(dict(id=oid,analyzer_id=analyzer,source=dict(start=max(0,round(a*source_rate)),end=min(source_frames,max(1,round(b*source_rate)))),confidence_millionths=round(confidence*1e6),uncertainty='Unreviewed machine estimate and explicit producer arrangement choice',value=value))
    observe('pulse',0,duration,dict(kind='tempo',milli_bpm=round(bpm*1000)))
    observe('direction',0,duration,dict(kind='rhythm',label='Producer-proposed three-beat piano accompaniment'))
    for i,(bar,index) in enumerate(zip(bars,path)):
        root,quality,pcs=bank[index];bar.update(root=root,quality=quality,pitch_classes=pcs,symbol=names[root]+('m' if quality=='minor' else ''),template_similarity=float(scores[i,index]))
        observe('chord-'+str(i),bar['start'],bar['end'],dict(kind='harmony',symbol=bar['symbol']),min(.65,max(.1,float(scores[i,index])*.6)))
    analysis=dict(schema='reel.music-analysis.v0.1',analysis_id=aid,source=parent['source'],analyzers=[dict(id=analyzer,adapter='librosa-beat-chroma-proposed-piano',version=librosa.__version__,model_revision='no-pretrained-model',parameters_sha256=descriptor('parameters.json')['sha256'],license='librosa ISC; source dependencies keep their own licenses',network_policy='denied')],stems=[],observations=obs,limitations=['Triad templates omit inversions, extensions and ambiguous harmony.','Three-beat meter and downbeat phase are producer proposals requiring listening.','Vocal estimate contains possible separation bleed; no original multitrack claim.'],review=dict(status='not-reviewed',required_roles=ROLES,decision_refs=[]))
    write('harmony-analysis.json',analysis);ar=native(request,'music-analysis-validate','harmony-analysis.json')
    def provenance(oid,reason):return dict(state='inferred',evidence_refs=[dict(analysis_id=aid,observation_id=oid)],rationale=reason,correction_ref=None)
    model=copy.deepcopy(parent);model['model_id']=request['arrangement_id'];model['analyses']=[dict(manifest='harmony-analysis.json',manifest_sha256=ar['manifest_sha256'],contract_sha256=ar['contract_sha256'],analysis_id=aid)]
    usq=round(60e6/bpm);ppq=model['musical_timebase']['pulses_per_quarter'];tick=lambda s:round(s*1e6/usq*ppq);model['duration_ticks']=tick(duration)
    model['authority']=dict(model['authority'],artifact_id=request['arrangement_id'],content_sha256=request['authority']['sha256'])
    model['tempo_map']=[dict(tick=0,microseconds_per_quarter=usq,provenance=provenance('pulse','Proposed piano pulse; source-clock note placements retain detected local timing'))]
    model['meter_map']=[dict(tick=0,numerator=3,denominator=4,provenance=provenance('direction','Producer waltz direction, not an approved source transcription'))]
    model['form']=[dict(id='full-recording',label='Full source extent',range=dict(start=0,end=tick(duration)),provenance=provenance('pulse','Exact complete Golden duration'))]
    notes=[];previous=None
    for i,bar in enumerate(bars):
        voiced=voicing(bar['root'],bar['pitch_classes'],previous);previous=voiced
        for k,index in enumerate(bar['beat_indices']):
            start=max(0,float(grid[index]));end=min(duration,float(grid[index+1]));length=end-start
            if length<.08:continue
            pitches=[36+bar['root']] if k==0 else voiced
            for voice,pitch in enumerate(pitches,1):
                attack=start+(.008*(voice-1) if k else 0);release=min(bar['end'],attack+length*(.8 if k==0 else .68))
                if release<=attack:continue
                notes.append(dict(id=f'piano-{i:03}-{k}-{voice}',voice=voice,start_tick=tick(attack),duration_ticks=max(1,tick(release)-tick(attack)),midi_note=pitch,velocity=(58 if k==0 else 47 if k==1 else 43),provenance=provenance('chord-'+str(i),'Producer piano voicing from inferred triad; exact recorded beat-clock placement')))
    notes.sort(key=lambda n:(n['start_tick'],n['voice'],n['midi_note']))
    model['parts']=[dict(id='piano-accompaniment',role='harmony',name='Piano',notes=notes)];model['harmony']=[dict(id='harmony-'+str(i),range=dict(start=tick(b['start']),end=tick(b['end'])),symbol=b['symbol'],provenance=provenance('chord-'+str(i),'Unreviewed triad-template harmony proposal')) for i,b in enumerate(bars)]
    model['lead_sheet']=None;model.pop('piano_vocal_score',None);model['rhythm_cells']=[];model['hooks']=[];model['expressive_timing']=[]
    model['unknowns']=analysis['limitations']+['Accompaniment proposal is not a completed melody transcription or approved arrangement.']
    write('arranged-model.json',model);checks=dict(analysis=ar,model=native(request,'music-model-validate','arranged-model.json'))
    native(request,'music-score-export-plan','arranged-model.json','--output-path','score-plan.json');native(request,'music-score-export-render','score-plan.json','arranged-model.json','--output-dir','native-score')
    checks['export']=native(request,'music-score-export-check','native-score/receipt.json','score-plan.json','arranged-model.json','native-score')
    write('native-checks.json',checks);write('harmony-evidence.json',dict(parameters=parameters,bars=bars,beat_grid_seconds=grid.tolist(),finished_arrangement=False))
    for name in ['score.mid','score.musicxml','rehearsal-guide.wav','receipt.json']:os.link(Path('native-score')/name,name)
    print('NATIVE PIANO PROPOSAL',len(notes),'notes',len(bars),'bars',bpm,'BPM',flush=True)


def sampled_piano(request,ms,env,expected):
    """Use the score app only as a sample source, never as the master clock."""
    import numpy as np
    import soundfile as sf
    import pretty_midi
    pitches=sorted({n['pitch'] for n in expected});bank=pretty_midi.PrettyMIDI(initial_tempo=120)
    instrument=pretty_midi.Instrument(program=0)
    for i,pitch in enumerate(pitches):instrument.notes.append(pretty_midi.Note(velocity=64,pitch=pitch,start=i*6.,end=i*6.+3.))
    bank.instruments.append(instrument);bank.write('sample-bank.mid')
    for output in ['piano-raw.wav','renderer-roundtrip.mid']:
        result=subprocess.run([str(ms),'--sound-profile','MS Basic','-f','-o',output,'sample-bank.mid'],capture_output=True,text=True,encoding='utf-8',env=env,timeout=600)
        Path(output+'.log').write_text(result.stdout+'\n'+result.stderr,encoding='utf-8')
        if result.returncode or not Path(output).exists():raise RuntimeError('Piano sample renderer failed: '+result.stderr)
    actual=midi_events('renderer-roundtrip.mid')
    if len(actual)!=len(pitches) or sorted(n['pitch'] for n in actual)!=pitches:raise ValueError('Sound renderer changed calibration pitches/count')
    audio,rate=sf.read('piano-raw.wav',dtype='float32',always_2d=True)
    if not np.isfinite(audio).all() or float(np.max(np.abs(audio)))<1e-4:raise ValueError('Silent/invalid piano sound bank')
    samples={};bank_ledger=[]
    for note in actual:
        a=round(note['start']*rate);hold_end=round(note['end']*rate)
        probe=np.max(np.abs(audio[a:min(len(audio),a+round(.6*rate))]),axis=1)
        if not len(probe) or float(probe.max())<1e-5:raise ValueError('Piano calibration note is missing audible sound')
        active=np.flatnonzero(probe>max(1e-6,float(probe.max())*.001))
        if not len(active):raise ValueError('Unresolved piano sample attack')
        attack=a+int(active[0]);sample=audio[attack:hold_end].copy()
        samples[note['pitch']]=sample
        bank_ledger.append(dict(pitch=note['pitch'],renderer_start_seconds=note['start'],audio_attack_frame=attack,held_sample_frames=len(sample)))
    frames=round(request['duration_seconds']*rate);piano=np.zeros((frames,2),dtype=np.float32);ledger=[]
    release=round(.22*rate)
    for n in expected:
        start=round(n['start']*rate);end=round(n['end']*rate);held=end-start
        if not 0<=start<end<=frames:raise ValueError('Piano schedule exceeds exact source duration')
        length=min(held+release,frames-start);sample=samples[n['pitch']]
        if len(sample)<length:raise ValueError('Piano calibration hold is too short for a source note')
        amplitude=(n.get('velocity',64)/64)**1.5
        chunk=sample[:length].copy()*amplitude
        if length>held:
            tail=length-held;envelope=np.linspace(1.,0.,tail,dtype=np.float32)**2;chunk[held:]*=envelope[:,None]
        piano[start:start+length]+=chunk
        ledger.append(dict(pitch=n['pitch'],start_frame=start,key_release_frame=end,release_tail_frames=length-held))
    scheduled=[dict(pitch=n['pitch'],start=n['start_frame']/rate,end=n['key_release_frame']/rate) for n in ledger]
    audit=audit_notes(expected,scheduled,1/rate)
    write('sample-bank-evidence.json',dict(schema='reel.piano-sample-clock-evidence.v1',method='Installed MS Basic piano samples; note-clock scheduling from exact native MIDI',bank=bank_ledger,schedule=ledger,sample_rate=rate,bank_pitch_count_verified=True,source_clock_audit=audit,velocity_policy='Fixed-64 piano sample scaled by (native_velocity/64)^1.5',release_policy='220ms quadratic key-release tail; no pedal',human_musical_review=False))
    return piano,rate,audit


def render(request):
    import numpy as np
    import soundfile as sf
    import pretty_midi
    from scipy.signal import resample_poly
    import math
    checked('score.mid',request['score_midi']);checked('vocals.wav',request['vocal_estimate'])
    input_score='score.mid'
    if request.get('score_musicxml'):
        checked('score.musicxml',request['score_musicxml']);input_score='score.musicxml'
    ms=checked(request['musescore']['path'],request['musescore']);checked(request['soundfont']['path'],request['soundfont']);ffmpeg=checked(request['ffmpeg']['path'],request['ffmpeg'])
    env=dict(os.environ)
    if os.name!='nt':env['QT_QPA_PLATFORM']='offscreen'
    else:env.pop('QT_QPA_PLATFORM',None)
    expected=midi_events('score.mid')
    if request.get('renderer_method')=='retained-piano-stem':
        checked('piano-stem.wav',request['piano_input']);checked('prior-render-receipt.json',request['prior_render_receipt'])
        prior=read('prior-render-receipt.json')
        if prior['native_midi']!=request['score_midi'] or prior['source_vocal']!=request['vocal_estimate'] or not prior['note_clock_audit']['passed']:raise ValueError('Prior piano source/clock binding differs')
        audit=prior['note_clock_audit'];piano,rate=sf.read('piano-stem.wav',dtype='float32',always_2d=True)
    elif request.get('renderer_method')=='native-clock-sample-bank':
        # Native note velocities are already source-bound in the editable score.
        import mido
        midi=mido.MidiFile('score.mid');velocities={};tempo=500000;seconds=0.
        for message in mido.merge_tracks(midi.tracks):
            seconds+=mido.tick2second(message.time,midi.ticks_per_beat,tempo)
            if message.type=='set_tempo':tempo=message.tempo
            elif message.type=='note_on' and message.velocity:velocities[(message.note,round(seconds,5))]=message.velocity
        for n in expected:n['velocity']=velocities[(n['pitch'],round(n['start'],5))]
        piano,rate,audit=sampled_piano(request,ms,env,expected)
    else:
        for output in ['piano-raw.wav','renderer-roundtrip.mid']:
            result=subprocess.run([str(ms),'--sound-profile','MS Basic','-f','-o',output,input_score],capture_output=True,text=True,encoding='utf-8',env=env,timeout=600)
            Path(output+'.log').write_text(result.stdout+'\n'+result.stderr,encoding='utf-8')
            if result.returncode or not Path(output).exists():raise RuntimeError('Piano sound renderer failed: '+result.stderr)
        actual=midi_events('renderer-roundtrip.mid');audit=audit_notes(expected,actual,request.get('clock_tolerance_seconds',.035))
        piano,rate=sf.read('piano-raw.wav',dtype='float32',always_2d=True)
    vocals,vrate=sf.read('vocals.wav',dtype='float32',always_2d=True)
    if rate!=vrate:
        divisor=math.gcd(rate,vrate);piano=resample_poly(piano,vrate//divisor,rate//divisor);rate=vrate
    frames=len(vocals);piano=np.pad(piano,((0,max(0,frames-len(piano))),(0,0)))[:frames]
    if not np.isfinite(piano).all() or float(np.sqrt(np.mean(piano**2)))<1e-5:raise ValueError('Silent or invalid piano proof')
    if piano.shape[1]!=2 or vocals.shape[1]!=2:raise ValueError('Expected stereo piano and retained vocal estimate')
    piano_gain=float(request.get('piano_target_rms',0))/float(np.sqrt(np.mean(piano**2))) if request.get('piano_target_rms') else .65
    vocal_gain=.85;mix=piano*piano_gain+vocals*vocal_gain
    peak=float(np.max(np.abs(mix)));master_gain=min(1,.89/max(peak,1e-9));piano*=piano_gain*master_gain;vocals_out=vocals*vocal_gain*master_gain;mix=piano+vocals_out
    sf.write('instrumental.wav',piano,rate,subtype='PCM_24');sf.write('with-vocals.wav',mix,rate,subtype='PCM_24');sf.write('vocal-stem.wav',vocals_out,rate,subtype='PCM_24')
    for name in ['instrumental','with-vocals']:
        subprocess.run([str(ffmpeg),'-v','error','-nostdin','-i',name+'.wav','-c:a','libmp3lame','-b:a','256k',name+'.mp3'],check=True,capture_output=True)
    receipt=dict(schema='reel.local-piano-arrangement-render.v1',source_vocal=request['vocal_estimate'],native_midi=request['score_midi'],note_clock_audit=audit,renderer_method=request.get('renderer_method','direct-score-export'),renderer=request['musescore'],soundfont=request['soundfont'],sample_rate=rate,frames=frames,duration_seconds=frames/rate,piano_gain=piano_gain*master_gain,vocal_gain=vocal_gain*master_gain,peak=float(np.max(np.abs(mix))),vocal_retimed=False,vocal_regenerated=False,vocal_pitched=False,instrumental_contains_added_vocal=False,arrangement_selected=False,public_release=False,limitations=['Separation estimate may retain original accompaniment bleed.','Piano harmony and waltz direction require musical listening review.'])
    if request.get('piano_input'):receipt.update(piano_input=request['piano_input'],prior_render_receipt=request['prior_render_receipt'])
    write('render-receipt.json',receipt);print('FULL PIANO AUDIO PROOF VERIFIED',frames/rate,'seconds',flush=True)


def plan(request):
    checked('arranged-model.json',request['arranged_model']);checked('harmony-analysis.json',request['harmony_analysis'])
    model=read('arranged-model.json');analysis=read('harmony-analysis.json');report=native(request,'music-model-validate','arranged-model.json')
    targets={}
    for field,prefix,key in [('tempo_map','tempo','tick'),('meter_map','meter','tick'),('form','form','id'),('harmony','harmony','id')]:
        for item in model[field]:targets[prefix+':'+str(item[key])]=item['provenance']
    for part in model['parts']:
        for note in part['notes']:targets['note:'+note['id']]=note['provenance']
    dispositions=[]
    for observation in analysis['observations']:
        mapped=[dict(target_ref=key,state=provenance['state'],rationale=provenance['rationale'],correction_ref=None) for key,provenance in targets.items() if dict(analysis_id=analysis['analysis_id'],observation_id=observation['id']) in provenance['evidence_refs']]
        if not mapped:raise ValueError('Unmapped piano analysis observation requires an explicit disposition')
        dispositions.append(dict(analysis_id=analysis['analysis_id'],observation_id=observation['id'],outcome=dict(kind='mapped',targets=mapped)))
    draft=dict(schema='reel.music-model-draft.v0.1',draft_id=request['arrangement_id']+'-inferred-source',model=dict(manifest='arranged-model.json',manifest_sha256=report['manifest_sha256'],contract_sha256=report['contract_sha256'],model_id=model['model_id']),authority=model['authority'],dispositions=dispositions,review=model['review'])
    write('model-draft.json',draft);dr=native(request,'music-model-draft-check','model-draft.json')
    arrangement=dict(schema='reel.music-arrangement-plan.v0.1',arrangement_id=request['arrangement_id'],model_draft=dict(manifest='model-draft.json',manifest_sha256=dr['manifest_sha256'],contract_sha256=dr['contract_sha256'],draft_id=draft['draft_id']),direction=dict(label='Gentle full-length piano waltz proof',objective='Private accompaniment proposal against the exact full Golden; original singing is reused separately',constraints=['Complete source duration; no vocal retiming or regeneration','Piano harmony and three-beat direction remain producer proposals','Musical listening and selection remain pending'],decision=dict(artifact_id='producer-arrangement-direction',sha256=request['authority']['sha256'])),ensemble=dict(maximum_instruments=1,instruments=[dict(id='piano',family='acoustic-piano',function='Gentle bass and close-voiced chord accompaniment',midi_low=21,midi_high=108,maximum_simultaneous_notes=10,techniques=['three-beat accompaniment','close triad voicing'])]),element_dispositions=[dict(model_target_id=key,action='preserve',rationale='Preserve exact unreviewed proposal for the bounded sound-render proof',decision=None) for key in targets],part_assignments=[dict(source_part_id=part['id'],action='preserve',instrument_ids=['piano'],rationale='Keep proposed piano part intact',decision=None) for part in model['parts']],note_mappings=[dict(id='mapping-'+note['id'],source_note_id=note['id'],instrument_id='piano',action='preserve',start_tick=note['start_tick'],duration_ticks=note['duration_ticks'],midi_note=note['midi_note'],velocity=note['velocity'],rationale='Exact native note timing, pitch and velocity',decision=None) for part in model['parts'] for note in part['notes']],candidate_gate=dict(required_checks=['exact-plan-binding','model-inheritance','range-and-polyphony','editable-score-round-trip','audible-comparison','human-recognition','human-selection']),authority=model['authority'],review=model['review'])
    write('arrangement-plan.json',arrangement);ar=native(request,'music-arrangement-plan-check','arrangement-plan.json')
    write('plan-checks.json',dict(model=report,model_draft=dr,arrangement_plan=ar,human_musical_review=False));print('NATIVE ARRANGEMENT PLAN VERIFIED',ar['mapped_notes'],flush=True)


def review_page(title,original,vocal,piano,midi,musicxml,duration_seconds=120,original_label="selected recording"):
    """Server-free review with relative links suitable for the private shelf."""
    import html
    esc=html.escape
    if not isinstance(duration_seconds,(int,float)) or not 0<duration_seconds<86400:raise ValueError("Invalid review duration")
    clock=f"{int(duration_seconds)//60}:{int(duration_seconds)%60:02d}"
    cards=''.join(f'<section><h2>{esc(label)}</h2><p>{esc(description)}</p><audio controls preload="metadata" src="{esc(path,quote=True)}"></audio><p><a href="{esc(path,quote=True)}" download>Download MP3</a></p></section>' for label,description,path in [('Piano + singing',f'Full {clock} listening proof using the selected recording’s vocal estimate.',vocal),('Piano accompaniment',f'Full {clock} instrumental backing for the same singing and timing.',piano),('Original Golden',f'Your {original_label}, for comparison.',original)])
    return f'''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{esc(title)} - Piano proof</title><style>body{{font:18px/1.5 system-ui;max-width:850px;margin:35px auto;padding:0 20px;background:#f7f5ed;color:#26352d}}h1{{font:42px Georgia,serif;margin-bottom:5px}}section{{padding:20px 24px;margin:22px 0;background:white;border:1px solid #d7ded5;border-radius:12px}}audio{{width:100%}}a{{color:#1a654e}}.tag{{font-size:14px;letter-spacing:.06em;text-transform:uppercase;color:#607363}}footer{{font-size:15px;color:#536557}}</style><p class="tag">Private listening proof · full length · {clock}</p><h1>{esc(title)}</h1><p>A gentle piano waltz proposal. Listen for the harmony under the singing, the balance, and the ending.</p>{cards}<section><h2>Editable accompaniment</h2><p><a href="{esc(musicxml,quote=True)}">MusicXML</a> · <a href="{esc(midi,quote=True)}">MIDI</a></p><p>The instrumental file is an accompaniment backing; it does not add a piano lead melody.</p></section><footer><p>Musical review and selection are pending. The vocal estimate may retain faint original instruments from separation. The selected Golden remains unchanged.</p></footer><script>document.querySelectorAll('audio').forEach(a=>a.addEventListener('play',()=>document.querySelectorAll('audio').forEach(other=>{{if(other!==a)other.pause()}})));</script></html>'''


def main():
    offline();request=read('arrangement.json')
    if request['schema']!='reel.local-piano-arrangement-request.v1':raise ValueError('Unsupported explicit arrangement request')
    if request['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Private local proof only')
    for row in request.get('runtime_files',[]):checked(row['path'],row)
    import importlib.metadata
    for name,version in request.get('runtime_packages',{}).items():
        if importlib.metadata.version(name)!=version:raise ValueError('Installed arrangement runtime version changed: '+name)
    if request['stage']=='analysis':analyze(request)
    elif request['stage']=='render':render(request)
    elif request['stage']=='plan':plan(request)
    else:raise ValueError('Unsupported stage')


if __name__=='__main__':main()
