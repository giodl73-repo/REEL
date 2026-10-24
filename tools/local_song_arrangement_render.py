"""Source-clock arrangement proposals and offline General MIDI sample rendering.

Private project requests own the style definitions and source locks. Native
REEL analysis/model/draft/plan and score exports own semantic validation. No
singing is generated, repitched or retimed; all musical proposals need listening.
"""
import copy
import json
import math
import os
from pathlib import Path
import subprocess
import zipfile
import xml.etree.ElementTree as ET
from fractions import Fraction
from local_song_backend import checked, descriptor, offline, read, write
from local_song_piano_arrangement import native, midi_events, audit_notes, voicing


def clean_notes(notes, duration):
    """Explicitly release a repeated key before its next attack."""
    result=[];last={}
    for n in sorted(notes,key=lambda x:(x['start_tick'],x['midi_note'],x['id'])):
        n=copy.deepcopy(n);pitch=n['midi_note'];start=n['start_tick']
        if pitch in last:
            prior=last[pitch]
            if prior['start_tick']==start:continue
            prior['duration_ticks']=min(prior['duration_ticks'],start-prior['start_tick'])
        n['duration_ticks']=min(n['duration_ticks'],duration-start)
        if n['duration_ticks']>0:result.append(n);last[pitch]=n
    return sorted(result,key=lambda n:(n['start_tick'],n['voice'],n['midi_note'],n['id']))


def native_parts(path):
    """Read native per-part identity and ticks without merging equal channels."""
    import mido
    midi=mido.MidiFile(path);result={}
    for track in midi.tracks:
        tick=0;part=None;active={};notes=[];port=0
        for message in track:
            tick+=message.time
            if message.type=='track_name' and message.name.startswith('REEL_PART:'):
                part=json.loads(message.name[len('REEL_PART:'):])[0]
            elif message.type=='midi_port':port=message.port
            elif message.type=='note_on' and message.velocity:
                key=(port,message.channel,message.note)
                if key in active:raise ValueError('Native MIDI repeats an unreleased key')
                active[key]=(tick,message.velocity)
            elif message.type=='note_off' or message.type=='note_on' and not message.velocity:
                key=(port,message.channel,message.note)
                if key not in active:raise ValueError('Unmatched native MIDI release')
                start,velocity=active.pop(key);notes.append((start,tick-start,message.note,velocity,message.channel+1))
        if active:raise ValueError('Native MIDI leaves held notes')
        if part:
            if part in result:raise ValueError('Duplicate native MIDI part')
            result[part]=sorted(notes)
    return result


def same_preset_sample(samples,pitch,required,rate,maximum_semitones):
    """Extend a preset's edge keys without changing the requested MIDI pitch."""
    import numpy as np
    from scipy.signal import resample_poly
    choices=[]
    for key,sample in samples.items():
        source=int(key[1:]);distance=abs(source-pitch)
        if distance>maximum_semitones:continue
        ratio=Fraction(2**((source-pitch)/12)).limit_denominator(10000)
        if len(sample)*float(ratio)>=math.ceil(required*rate):choices.append((distance,source,key,ratio))
    if not choices:raise ValueError('No same-preset calibration key has the declared range and usable duration for pitch '+str(pitch))
    distance,source,key,ratio=min(choices);result=resample_poly(samples[key],ratio.numerator,ratio.denominator,axis=0).astype(np.float32)
    if len(result)<math.ceil(required*rate) or not np.isfinite(result).all():raise ValueError('Invalid derived same-preset key')
    return result,dict(pitch=pitch,method='same-preset-key-resampling',source_pitch=source,ratio_numerator=ratio.numerator,ratio_denominator=ratio.denominator,ratio_cents_error=1200*math.log2(float(ratio)/(2**((source-pitch)/12))),sample_frames=len(result),required_sample_seconds=required,source_samples_unchanged=True)


def bank(q):
    import numpy as np
    import pretty_midi
    import soundfile as sf
    instrument=q['instrument'];pitches=q['pitches'];ms=checked(q['musescore']['path'],q['musescore']);checked(q['soundfont']['path'],q['soundfont'])
    midi=pretty_midi.PrettyMIDI(initial_tempo=120);part=pretty_midi.Instrument(program=instrument['program'],is_drum=instrument.get('drums',False))
    cursor=0.
    for p in pitches:
        hold=q.get('pitch_holds',{}).get(str(p),3.)
        part.notes.append(pretty_midi.Note(velocity=64,pitch=p,start=cursor,end=cursor+hold));cursor+=hold+3.
    midi.instruments.append(part);midi.write('calibration.mid');env=dict(os.environ);env.pop('QT_QPA_PLATFORM',None)
    calibration_input='calibration.mid'
    if q.get('program_policy')=='explicit-native-channel':
        result=subprocess.run([str(ms),'--sound-profile','MS Basic','-f','-o','calibration-import.mscx','calibration.mid'],env=env,capture_output=True,text=True,encoding='utf-8',timeout=600);Path('calibration-import.log').write_text(result.stdout+'\n'+result.stderr,encoding='utf-8')
        if result.returncode:raise RuntimeError('Existing sample renderer could not create its editable calibration score')
        tree=ET.parse('calibration-import.mscx');channels=tree.getroot().findall('.//Part/Instrument/Channel')
        if not channels:raise ValueError('No explicit native calibration instrument channel')
        changes=[]
        for channel in channels:
            program=channel.find('program');prior=program.attrib.get('value') if program is not None else '0'
            if program is None:program=ET.SubElement(channel,'program')
            program.set('value',str(instrument['program']));changes.append(dict(channel=channel.attrib.get('name','normal'),imported_program=prior,requested_program=instrument['program']))
        tree.write('calibration.mscx',encoding='utf-8',xml_declaration=True);write('calibration-programs.json',dict(policy='Explicit requested General MIDI program in the existing app native instrument channels',changes=changes,notes_changed=False));calibration_input='calibration.mscx'
    for target in ['calibration.wav','roundtrip.mid']:
        result=subprocess.run([str(ms),'--sound-profile','MS Basic','-f','-o',target,calibration_input],env=env,capture_output=True,text=True,encoding='utf-8',timeout=600);Path(target+'.log').write_text(result.stdout+'\n'+result.stderr,encoding='utf-8')
        if result.returncode:raise RuntimeError('Existing sample renderer failed: '+result.stderr)
    actual=midi_events('roundtrip.mid')
    if sorted(n['pitch'] for n in actual)!=sorted(pitches):raise ValueError('Calibration changed pitches/count')
    import mido
    programs=sorted({m.program for t in mido.MidiFile('roundtrip.mid').tracks for m in t if m.type=='program_change'})
    if not instrument.get('drums') and instrument['program'] not in programs and not (instrument['program']==0 and not programs):raise ValueError('Calibration changed requested instrument program: '+str(programs))
    audio,rate=sf.read('calibration.wav',dtype='float32',always_2d=True);samples={};ledger=[];silent=[]
    for index,n in enumerate(actual):
        a=round(n['start']*rate);b=round(n['end']*rate)
        if instrument.get('drums'):
            # Percussion note-offs do not bound the recorded one-shot decay.
            # Retain real calibration audio before the next isolated attack.
            next_start=actual[index+1]['start'] if index+1<len(actual) else len(audio)/rate
            b=min(len(audio),round(min(n['start']+q.get('pitch_holds',{}).get(str(n['pitch']),3.),next_start-.1)*rate))
        probe=np.max(np.abs(audio[a:min(len(audio),a+round(.8*rate))]),axis=1)
        if not len(probe) or probe.max()<1e-5:
            policy=q.get('silent_pitch_policy',{})
            if not instrument.get('drums') and policy.get('method')=='same-preset-key-resampling':silent.append(n);continue
            raise ValueError('Silent requested instrument calibration for pitch '+str(n['pitch']))
        active=np.flatnonzero(probe>max(1e-6,float(probe.max())*.001));attack=a+int(active[0]);sample=audio[attack:b].copy()
        required=q.get('required_sample_seconds',{}).get(str(n['pitch']),q.get('pitch_holds',{}).get(str(n['pitch']),3.)-.25)
        if len(sample)<round(required*rate):raise ValueError('Sample hold too short for pitch '+str(n['pitch'])+': '+str(len(sample)/rate)+' < '+str(required))
        samples['p'+str(n['pitch'])]=sample;ledger.append(dict(pitch=n['pitch'],method='native-rendered',attack_frame=attack,sample_frames=len(sample),renderer_start_seconds=n['start'],renderer_key_end_seconds=n['end'],capture_end_frame=b,required_sample_seconds=required,capture_hold_seconds=q.get('pitch_holds',{}).get(str(n['pitch']),3.)))
    direct=dict(samples)
    for n in silent:
        maximum=q['silent_pitch_policy']['maximum_semitones']
        if not isinstance(maximum,int) or not 1<=maximum<=12:raise ValueError('Same-preset extension is bounded to one octave')
        required=q.get('required_sample_seconds',{}).get(str(n['pitch']),q.get('pitch_holds',{}).get(str(n['pitch']),3.)-.25);sample,row=same_preset_sample(direct,n['pitch'],required,rate,maximum);samples['p'+str(n['pitch'])]=sample;ledger.append(row)
    np.savez_compressed('samples.npz',**samples);write('bank-evidence.json',dict(schema='reel.instrument-sample-bank.v1',instrument=instrument,sample_rate=rate,calibration_pitches=pitches,ledger=ledger,renderer=q['musescore'],soundfont=q['soundfont'],roundtrip_programs=programs,pitch_count_verified=True,human_timbre_review=False));print('SAMPLE BANK VERIFIED',instrument['name'],len(pitches),flush=True)


def assigned_midi(source,programs,target):
    import mido
    original=native_parts(source);midi=mido.MidiFile(source);instruments={p['id']:p for p in programs};channels=[*range(9),*range(10,16)];next_voice=0;routes={};parts=[]
    for track in midi.tracks:
        identity=next((json.loads(m.name[len('REEL_PART:'):])[0] for m in track if m.type=='track_name' and m.name.startswith('REEL_PART:')),None)
        if identity:
            voices=sorted({m.channel for m in track if not m.is_meta and hasattr(m,'channel')});parts.append((track,identity,voices))
            if not instruments[identity]['drums']:
                for voice in voices:routes[(identity,voice)]=(next_voice//15,channels[next_voice%15]);next_voice+=1
    drum_port=(next_voice+14)//15
    for track,identity,voices in parts:
        if instruments[identity]['drums']:
            for voice in voices:routes[(identity,voice)]=(drum_port,9);drum_port+=1
    if any(port>127 for port,channel in routes.values()):raise ValueError('Player MIDI exceeds bounded independent voice ports')
    for track in midi.tracks:
        identity=next((json.loads(m.name[len('REEL_PART:'):])[0] for m in track if m.type=='track_name' and m.name.startswith('REEL_PART:')),None)
        if identity:
            part=instruments[identity];rewritten=[]
            for voice in sorted({m.channel for m in track if not m.is_meta and hasattr(m,'channel')}):
                port,channel=routes[(identity,voice)];rewritten.extend([mido.MetaMessage('midi_port',port=port,time=0),mido.Message('program_change',channel=channel,program=part['program'],time=0)])
            for message in track:
                if not message.is_meta and hasattr(message,'channel'):
                    port,channel=routes[(identity,message.channel)];rewritten.append(mido.MetaMessage('midi_port',port=port,time=message.time));rewritten.append(message.copy(channel=channel,time=0))
                else:rewritten.append(message)
            track[:]=rewritten
    midi.save(target)
    sounding=lambda parts:{k:sorted(n[:4] for n in rows) for k,rows in parts.items()}
    if sounding(native_parts(target))!=sounding(original):raise ValueError('Player channel/program assignment changed sounding notes or clocks')


def encode_fixed(ff,source,target,limit=.95):
    import numpy as np
    import soundfile as sf
    audio,rate=sf.read(source,dtype='float32',always_2d=True)
    subprocess.run([str(ff),'-v','error','-nostdin','-i',str(source),'-af',f'alimiter=limit={limit}:level=false:latency=true','-ar','44100','-ac','2','-c:a','libmp3lame','-b:a','192k',str(target)],check=True,capture_output=True)
    pcm=np.frombuffer(subprocess.check_output([str(ff),'-v','error','-i',str(target),'-f','f32le','-']),dtype='<f4')
    if rate!=44100 or len(pcm)!=len(audio)*2 or not np.isfinite(pcm).all():raise ValueError('Full fixed-edition decode/extent failed')
    return dict(frames=len(audio),sample_rate=44100,duration_seconds=len(audio)/44100,decoded_peak=float(np.max(np.abs(pcm))),input_peak=float(np.max(np.abs(audio))),input_rms=float(np.sqrt(np.mean(audio**2))),dynamic_loudnorm_applied=False,limiter_auto_level=False,limiter_limit=limit,limiter_latency_compensated=True,full_decode=True)


def layer(q):
    import numpy as np
    import soundfile as sf
    checked('model.json',q['model']);checked('score.mid',q['score_midi']);checked('programs.json',q['programs']);model=read('model.json');programs=read('programs.json');actual=native_parts('score.mid')
    rate=44100;frames=round(q['duration_seconds']*rate);audio=np.zeros((frames,2),dtype=np.float32);scale=model['tempo_map'][0]['microseconds_per_quarter']/1e6/model['musical_timebase']['pulses_per_quarter'];audits=[]
    by_id={p['id']:p for p in model['parts']}
    for instrument in programs:
        identity=instrument['id'];expected=sorted((n['start_tick'],n['duration_ticks'],n['midi_note'],n['velocity'],n['voice']) for n in by_id[identity]['notes'])
        if actual[identity]!=expected:raise ValueError('Native source model/MIDI note discrepancy')
        key=str(instrument['program'])+('-drums' if instrument['drums'] else '');binding=q['banks'][key];checked(binding['samples_path'],binding['samples']);checked(binding['evidence_path'],binding['evidence']);meta=read(binding['evidence_path'])
        if meta['sample_rate']!=rate or meta['instrument']['program']!=instrument['program'] or meta['instrument'].get('drums',False)!=instrument['drums']:raise ValueError('Wrong source instrument calibration')
        release=round(q.get('release_seconds',.22)*rate);ledger=[]
        with np.load(binding['samples_path'],allow_pickle=False) as samples:
            loaded={pitch:samples['p'+str(pitch)] for pitch in sorted({n[2] for n in expected})}
            for start,duration,pitch,velocity,voice in expected:
                a=round(start*scale*rate);b=round((start+duration)*scale*rate);held=b-a;size=min(held+release,frames-a)
                if not 0<=a<b<=frames:raise ValueError('Source note exceeds declared full edition clock')
                sample=loaded[pitch]
                if size>len(sample):raise ValueError('Calibrated hold too short for the measured note')
                chunk=sample[:size].copy()*(velocity/64)**1.5
                if size>held:chunk[held:]*=(np.linspace(1,0,size-held,dtype=np.float32)**2)[:,None]
                audio[a:a+size]+=chunk;ledger.append(dict(pitch=pitch,start=a/rate,end=b/rate))
        audits.append(dict(part=identity,program=instrument['program'],**audit_notes([dict(pitch=p,start=s*scale,end=(s+d)*scale) for s,d,p,v,voice in expected],ledger,1/rate)))
    if not np.isfinite(audio).all() or np.sqrt(np.mean(audio**2))<1e-6:raise ValueError('Silent/invalid recording-derived sound layer')
    sf.write('layer-unfiltered.wav',audio,rate,subtype='FLOAT');ff=checked(q['ffmpeg']['path'],q['ffmpeg']);filters=q.get('filter') or 'anull'
    subprocess.run([str(ff),'-v','error','-nostdin','-i','layer-unfiltered.wav','-af',filters+',apad,atrim=end_sample='+str(frames),'-c:a','pcm_f32le','layer.wav'],check=True,capture_output=True)
    treated,tr=sf.read('layer.wav',dtype='float32',always_2d=True)
    if tr!=rate or treated.shape!=audio.shape or not np.isfinite(treated).all():raise ValueError('Layer filter changed declared extent')
    np.savez_compressed('layer-lossless.npz',audio=treated,sample_rate=rate);assigned_midi('score.mid',programs,'layer.mid');preview=encode_fixed(ff,'layer.wav','layer.mp3')
    write('layer-evidence.json',dict(schema='reel.recording-derived-layer-render.v1',source_model=q['model'],source_midi=q['score_midi'],programs=q['programs'],duration_seconds=len(treated)/rate,frames=frames,sample_rate=rate,part_clock_audits=audits,filter=filters,preview=preview,renderer_policy='Existing MS Basic calibration samples; exact native key clocks; fixed-64 samples scaled by (velocity/64)^1.5; no RMS normalization',lossless_float_peak=float(np.max(np.abs(treated))),musical_approval=False));print('SOURCE LAYER RENDERED',q['kind'],len(treated)/rate,flush=True)


def component_modes(component):
    modes=component.get('modes',['with_vocals','without_vocals'])
    if modes not in [['with_vocals','without_vocals'],['without_vocals']]:
        raise ValueError('Unsupported edition component mode scope')
    return modes


def edition(q):
    import numpy as np
    import soundfile as sf
    from local_song_recording_layers import arrangement_plan
    duration=q['duration_seconds'];rate=44100;frames=round(duration*rate);mix=np.zeros((frames,2),dtype=np.float32);without_extra=np.zeros_like(mix);model=None;programs=[];analyses=[];seen=set();component_evidence=[]
    for component in q['components']:
        checked(component['lossless_path'],component['lossless']);checked(component['model_path'],component['model']);checked(component['programs_path'],component['programs'])
        with np.load(component['lossless_path'],allow_pickle=False) as data:
            audio=data['audio'];sr=int(data['sample_rate'])
            if sr!=rate or audio.shape!=(frames,2):raise ValueError('Edition component clock differs')
            modes=component_modes(component)
            if modes==['without_vocals']:without_extra+=audio*component['gain']
            else:mix+=audio*component['gain']
        part_model=read(component['model_path']);part_programs=read(component['programs_path'])
        if model is None:model=copy.deepcopy(part_model);model['parts']=[]
        if part_model['source']!=model['source'] or part_model['duration_ticks']!=model['duration_ticks']:raise ValueError('Edition components use different source clocks')
        for binding in part_model['analyses']:
            if binding['analysis_id'] not in seen:analyses.append(binding);seen.add(binding['analysis_id'])
        model['parts'].extend(copy.deepcopy(part_model['parts']));programs.extend(part_programs);component_evidence.append(dict(kind=component['kind'],gain=component['gain'],lossless=component['lossless'],model=component['model'],modes=modes))
    if model is None:raise ValueError('No source-supported edition components')
    if len({p['id'] for p in model['parts']})!=len(model['parts']):raise ValueError('Duplicate edition source part')
    model['analyses']=analyses;model['model_id']=q['arrangement_id'];model['authority']=dict(model['authority'],artifact_id=q['arrangement_id'],content_sha256=q['authority']['sha256'])
    instruments={p['id']:p for p in programs}
    for binding in analyses:
        clock=dict(analysis_id=binding['analysis_id'],observation_id='display-clock')
        for field in ['tempo_map','meter_map','form']:
            for item in model[field]:
                if clock not in item['provenance']['evidence_refs']:item['provenance']['evidence_refs'].append(clock)
    write('arranged-model.json',model);mr=native(q,'music-model-validate','arranged-model.json');native(q,'music-score-export-plan','arranged-model.json','--output-path','score-plan.json');native(q,'music-score-export-render','score-plan.json','arranged-model.json','--output-dir','native-score');checks=dict(model=mr,score_export=native(q,'music-score-export-check','native-score/receipt.json','score-plan.json','arranged-model.json','native-score'));checks.update(arrangement_plan(q,model,'arranged-model.json',mr,programs,'edition'));write('native-checks.json',checks);write('edition-programs.json',programs)
    assigned_midi('native-score/score.mid',programs,'arrangement.mid')
    for name in ['score.mid','score.musicxml','receipt.json']:os.link(Path('native-score')/name,name)
    instrumental=q.get('source_mode')=='instrumental'
    if instrumental and np.any(without_extra):raise ValueError('Pure instrumental bindings must remain identical')
    if instrumental:
        if q.get('vocal_estimate') is not None:raise ValueError('Instrumental mode cannot bind an invented vocal stem')
        padded=np.zeros_like(mix)
    else:
        checked('vocals.wav',q['vocal_estimate']);vocal,vr=sf.read('vocals.wav',dtype='float32',always_2d=True)
        if vr!=rate or vocal.shape!=(round(q['source_duration_seconds']*rate),2):raise ValueError('Exact full-Golden vocal extent differs')
        padded=np.pad(vocal,((0,frames-len(vocal)),(0,0)))
    ff=checked(q['ffmpeg']['path'],q['ffmpeg']);metrics=[]
    for mode,audio in [('with_vocals',mix+padded),('without_vocals',mix+without_extra)]:
        if instrumental and mode=='without_vocals':
            for suffix in ['.mp3','.flac']:os.link('with_vocals'+suffix,mode+suffix)
            metrics.append(dict(metrics[0],mode=mode,identical_instrumental_binding=True));continue
        sf.write(mode+'-input.wav',audio,rate,subtype='FLOAT');metrics.append(dict(mode=mode,**encode_fixed(ff,mode+'-input.wav',mode+'.mp3')))
        subprocess.run([str(ff),'-v','error','-nostdin','-i',mode+'-input.wav','-af','alimiter=limit=0.95:level=false:latency=true','-c:a','flac','-sample_fmt','s32',mode+'.flac'],check=True,capture_output=True)
    write('edition-evidence.json',dict(schema='reel.existing-edition-render.v1',edition=q['style'],source_golden_lock=q['golden_lock'],source_duration_seconds=q['source_duration_seconds'],postroll_seconds=q['postroll_seconds'],duration_seconds=duration,source_mode=q.get('source_mode','sung'),source_vocal=q['vocal_estimate'],vocal_gain=None if instrumental else 1.0,instrumental_mode_bindings_identical=instrumental,vocal_regenerated=False,vocal_retimed=False,vocal_repitched=False,components=component_evidence,score_mode='without_vocals',mode_scoped_instrumental_melody=any(c['modes']==['without_vocals'] for c in component_evidence),modes=metrics,mastering=dict(mode='fixed',dynamic_loudnorm_applied=False,limiter_auto_level=False),musical_approval=False));print('BOTH EXISTING-SCHEMA EDITION MODES RENDERED',q['style'],duration,flush=True)
    with zipfile.ZipFile('edition-score-payloads.zip','w',compression=zipfile.ZIP_DEFLATED) as archive:
        for name in ['arranged-model.json','edition-programs.json','native-checks.json','edition-draft.json','edition-arrangement-plan.json','score-plan.json','edition-evidence.json','arrangement.mid']:
            archive.write(name)
        for path in sorted(Path('native-score').iterdir()):
            if path.is_file():archive.write(path)


def lossless_layer_export(q):
    import numpy as np
    import soundfile as sf
    source=checked('layer-lossless.npz',q['lossless'])
    with np.load(source,allow_pickle=False) as data:
        audio=data['audio'];rate=int(data['sample_rate'])
        if rate!=44100 or audio.shape!=(round(q['duration_seconds']*rate),2) or not np.isfinite(audio).all():raise ValueError('Lossless stem extent or samples differ')
        sf.write('layer.wav',audio,rate,subtype='FLOAT')
    restored,restored_rate=sf.read('layer.wav',dtype='float32',always_2d=True)
    if restored_rate!=rate or not np.array_equal(audio,restored):raise ValueError('Lossless stem export changed samples')
    write('layer-export-check.json',dict(source=q['lossless'],wav=descriptor('layer.wav'),sample_rate=rate,frames=len(audio),channels=2,float_peak=float(np.max(np.abs(audio))),samples_unchanged=True,gain_applied=False,clipped_to_integer_pcm=False,musical_approval=False))


def master(q):
    import numpy as np
    import soundfile as sf
    ff=checked(q['ffmpeg']['path'],q['ffmpeg']);receipts=[]
    for mode in ['with_vocals','without_vocals']:
        arrays=[];segments=[];cursor=0
        for item in q['tracks']:
            row=item[mode];path=checked(row['path'],row)
            raw=subprocess.check_output([str(ff),'-v','error','-i',str(path),'-ar','44100','-ac','2','-f','f32le','-']);audio=np.frombuffer(raw,dtype='<f4').reshape(-1,2)
            if not np.isfinite(audio).all():raise ValueError('Invalid continuous-master source')
            segments.append(dict(sequence=item['sequence'],slug=item['slug'],start_frame=cursor,frames=len(audio),source={k:row[k] for k in ['sha256','bytes']}));arrays.append(audio);cursor+=len(audio)
        combined=np.concatenate(arrays);sf.write(mode+'-input.wav',combined,44100,subtype='FLOAT');metrics=encode_fixed(ff,mode+'-input.wav',mode+'.mp3',limit=.84);receipts.append(dict(mode=mode,segments=segments,**metrics))
    write('master-evidence.json',dict(schema='reel.existing-edition-masters.v1',edition=q['style'],modes=receipts,source_segments_reused=True,new_intertrack_gap_seconds=0,mastering=dict(mode='fixed',limiter_auto_level=False,dynamic_loudnorm_applied=False),musical_approval=False));print('BOTH CONTINUOUS EDITION MASTERS VERIFIED',q['style'],flush=True)


def main():
    offline();q=read('render-request.json')
    if q['schema']!='reel.recording-derived-arrangement-render.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Explicit private existing-schema render request required')
    import importlib.metadata
    for name,version in q.get('runtime_packages',{}).items():
        if importlib.metadata.version(name)!=version:raise ValueError('Installed renderer package changed: '+name)
    if q['stage']=='batch':
        items=q['items']
        if not 1<=len(items)<=14:raise ValueError('A sound-render batch supports one bounded fourteen-track style')
        root=Path.cwd();completed=[]
        for item in items:
            directory=item['directory'];request=item['request']
            if Path(directory).name!=directory or directory in ['.','..'] or request['schema']!=q['schema'] or request['permissions']!=q['permissions']:raise ValueError('Invalid scoped render batch item')
            folder=root/directory;folder.mkdir(exist_ok=True);write(folder/'render-request.json',request);os.chdir(folder)
            try:
                if request['stage']=='layer':layer(request)
                elif request['stage']=='edition':edition(request)
                elif request['stage']=='lossless-layer-export':lossless_layer_export(request)
                else:raise ValueError('A sound batch only supports source layers or editions')
            finally:os.chdir(root)
            completed.append(dict(directory=directory,stage=request['stage']));print('BATCH TRACK VERIFIED',directory,len(completed),'of',len(items),flush=True)
        write('batch-report.json',dict(completed=completed,requested=len(items),musical_approval=False))
    elif q['stage']=='bank':bank(q)
    elif q['stage']=='layer':layer(q)
    elif q['stage']=='edition':edition(q)
    elif q['stage']=='master':master(q)
    elif q['stage']=='lossless-layer-export':lossless_layer_export(q)
    else:raise ValueError('Unknown existing-schema render stage')


if __name__=='__main__':main()
