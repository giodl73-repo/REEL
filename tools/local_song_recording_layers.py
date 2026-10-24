"""Native score/arrangement layers from measured model notes, not new chords.

The request supplies existing edition recipes. Recording-derived notes, guitar
treatment, filtered additional layers and final mixes remain separate assets.
"""
import copy
import hashlib
import json
import math
from pathlib import Path
import random
import zipfile
from local_song_backend import checked, descriptor, offline, read, write
from local_song_piano_arrangement import native


def arrangement_plan(q,model,model_name,model_report,programs,prefix):
    """Bind every existing native observation/target to an explicit draft/plan."""
    targets={}
    for field,label,key in [('tempo_map','tempo','tick'),('meter_map','meter','tick'),('form','form','id'),('harmony','harmony','id')]:
        for item in model[field]:targets[label+':'+str(item[key])]=item['provenance']
    for part in model['parts']:
        for note in part['notes']:targets['note:'+note['id']]=note['provenance']
    dispositions=[]
    for binding in model['analyses']:
        analysis=read(binding['manifest'])
        for obs in analysis['observations']:
            mapped=[dict(target_ref=k,state=p['state'],rationale=p['rationale'],correction_ref=None) for k,p in targets.items() if dict(analysis_id=analysis['analysis_id'],observation_id=obs['id']) in p['evidence_refs']]
            if not mapped:raise ValueError('Unmapped recording-derived observation')
            dispositions.append(dict(analysis_id=analysis['analysis_id'],observation_id=obs['id'],outcome=dict(kind='mapped',targets=mapped)))
    draft=dict(schema='reel.music-model-draft.v0.1',draft_id=model['model_id']+'-draft',model=dict(manifest=model_name,manifest_sha256=model_report['manifest_sha256'],contract_sha256=model_report['contract_sha256'],model_id=model['model_id']),authority=model['authority'],dispositions=dispositions,review=model['review']);draft_name=prefix+'-draft.json';write(draft_name,draft);dr=native(q,'music-model-draft-check',draft_name)
    instruments=[dict(id=p['id'],family='percussion' if p['drums'] else 'general-midi-'+str(p['program']),function='Recording-derived '+p['source_kind'],midi_low=0,midi_high=127,maximum_simultaneous_notes=128,techniques=['recording-derived notes','explicit source-clock timing']) for p in programs]
    plan=dict(schema='reel.music-arrangement-plan.v0.1',arrangement_id=model['model_id'],model_draft=dict(manifest=draft_name,manifest_sha256=dr['manifest_sha256'],contract_sha256=dr['contract_sha256'],draft_id=draft['draft_id']),direction=dict(label=prefix,objective='Existing-schema recording-derived layer/edition candidate',constraints=['No manufactured note duration','Unsupported source instruments omitted','Musical review pending'],decision=dict(artifact_id='producer-existing-schema',sha256=q['authority']['sha256'])),ensemble=dict(maximum_instruments=len(instruments),instruments=instruments),element_dispositions=[dict(model_target_id=k,action='preserve',rationale='Preserve the explicitly declared recording-derived proposal',decision=None) for k in targets],part_assignments=[dict(source_part_id=p['id'],action='preserve',instrument_ids=[p['id']],rationale='Exact source-supported instrument assignment',decision=None) for p in model['parts']],note_mappings=[dict(id='mapping-'+n['id'],source_note_id=n['id'],instrument_id=p['id'],action='preserve',start_tick=n['start_tick'],duration_ticks=n['duration_ticks'],midi_note=n['midi_note'],velocity=n['velocity'],rationale='Exact native declared note',decision=None) for p in model['parts'] for n in p['notes']],candidate_gate=dict(required_checks=['exact-plan-binding','model-inheritance','range-and-polyphony','editable-score-round-trip','audible-comparison','human-recognition','human-selection']),authority=model['authority'],review=model['review']);plan_name=prefix+'-arrangement-plan.json';write(plan_name,plan);pr=native(q,'music-arrangement-plan-check',plan_name)
    return dict(model_draft=dr,arrangement_plan=pr)


def guitar_treatment(notes,seed=7301):
    """Port the established named nylon-guitar treatment, with a change ledger."""
    rng=random.Random(seed);end=max(n['end'] for n in notes);groups=[];result=[];ledger=[]
    for note in sorted(notes,key=lambda n:(n['start'],n['pitch'])):
        if not groups or note['start']-groups[-1][0]['start']>.025:groups.append([note])
        else:groups[-1].append(note)
    prior={}
    for index,group in enumerate(groups):
        ordered=sorted(group,key=lambda n:n['pitch'],reverse=bool(index%2));spread=min(.024,.075/max(1,len(group)-1)) if len(group)>1 else 0
        for i,original in enumerate(ordered):
            note=copy.deepcopy(original);start=original['start'];note['start']=max(0,start+i*spread+rng.uniform(-.004,.004));phrase=math.sin(2*math.pi*start/8);beat=int(start*2)%4;register=(64-note['pitch'])*.12
            note['velocity']=max(52,min(108,round(79+8*phrase+(8 if beat==0 else 3 if beat==2 else 0)+register+rng.uniform(-3,3))))
            note['end']=max(note['start']+.09,min(end,note['start']+(original['end']-start)*1.10))
            if note['pitch'] in prior and prior[note['pitch']]['end']>note['start']-.025:
                p=prior[note['pitch']];p['end']=max(p['start']+.04,note['start']-.025)
            prior[note['pitch']]=note;result.append(note);ledger.append(dict(source_note=original['id'],original_start=original['start'],original_end=original['end'],treated_note=note))
    return sorted(result,key=lambda n:(n['start'],n['pitch'],n['id'])),ledger


def filter_layer(parts,definition):
    result=[];count=0
    for part in parts:
        drum=part['drums'];program=part['program']
        if drum and definition['drums'] or not drum and program in definition['programs']:
            selected=copy.deepcopy(part)
            if drum and definition.get('hand'):
                hand=[60,61,70,75,82]
                for i,n in enumerate(sorted(selected['notes'],key=lambda n:n['start'])):n['pitch']=hand[i%len(hand)];n['velocity']=max(42,min(88,round(n.get('velocity',100)*.72)))
            count+=len(selected['notes']);result.append(selected)
    return result,count


def notation_voices(notes,maximum=16):
    """Partition polyphony for MusicXML without changing any sounding event."""
    ends=[];result=[]
    for note in sorted(notes,key=lambda n:(n['start_tick'],n['midi_note'],n['id'])):
        voice=next((i for i,end in enumerate(ends) if end<=note['start_tick']),len(ends))
        if voice==len(ends):
            if len(ends)>=maximum:raise ValueError('Source polyphony exceeds native interchange voice capacity')
            ends.append(0)
        ends[voice]=note['start_tick']+note['duration_ticks'];item=copy.deepcopy(note);item['voice']=voice+1;result.append(item)
    return sorted(result,key=lambda n:(n['start_tick'],n['voice'],n['midi_note'],n['id']))


def build(q):
    checked('source.json',q['source']);base=read('source.json');duration=q['duration_seconds'];authority=q['authority'];timebase=dict(pulses_per_quarter=1000,rounding='half-away-from-zero')
    # Millisecond display ticks preserve the estimator's 10ms event clock.
    source=copy.deepcopy(base);source['source_id']=q['artifact_id']+'-recording-derived';source['musical_timebase']=timebase;source['authority']=dict(source['authority'],artifact_id=q['artifact_id'],content_sha256=authority['sha256'])
    raw=Path('source.pcm').read_bytes();tb=source['media']['timebase'];expected=tb['samples_per_channel']*tb['channels']*4
    if len(raw)!=expected or abs(tb['samples_per_channel']/tb['sample_rate_hz']-duration)>1/tb['sample_rate_hz']:raise ValueError('Original decoded PCM extent differs')
    tail=q['postroll_seconds'];padded=raw+b'\0'*round(tail*tb['sample_rate_hz']*tb['channels']*4);Path('layer-source.pcm').write_bytes(padded);pcm=descriptor('layer-source.pcm');source['media'].update(path='layer-source.pcm',sha256=pcm['sha256'],decoded_pcm_sha256=pcm['sha256']);tb['samples_per_channel']+=round(tail*tb['sample_rate_hz']);model_duration=duration+tail
    write('layer-source.json',source);sr=native(q,'music-source-validate','layer-source.json');binding=dict(manifest='layer-source.json',manifest_sha256=sr['manifest_sha256'],contract_sha256=sr['contract_sha256'],decoded_pcm_sha256=source['media']['decoded_pcm_sha256'])
    grouped={};ledgers={};summary=read('transcription-evidence.json')
    for kind in ['piano','guitar','band','voice']:
        if kind=='voice' and kind not in q['measured']:
            if q.get('source_mode')!='instrumental':raise ValueError('Only an explicitly instrumental source may omit a vocal layer')
            grouped[kind]=[];continue
        checked(kind+'-notes.json',q['measured'][kind]);data=read(kind+'-notes.json');groups={}
        if not data.get('validation_passed',False):raise ValueError('Failed source note validation cannot enter a playable layer')
        if data['source']!=(q['vocal_estimate'] if kind=='voice' else q['recording']):raise ValueError('Measured source differs from full Golden')
        for note in data['notes']:
            if note['end']>duration:raise ValueError('Measured layer exceeds source extent')
            name=note['instrument'];groups.setdefault(name,[]).append(dict(note,velocity=100))
        grouped[kind]=[dict(id=kind+'-'+name,source_kind=kind,name=name,program=data['programs'][name],drums=name=='drums',notes=notes) for name,notes in sorted(groups.items())]
    treated=[]
    for part in grouped['guitar']:
        notes,ledger=guitar_treatment(part['notes']);treated.append(dict(part,id='spanish-'+part['name'],program=24,notes=notes));ledgers[part['id']]=ledger
    grouped['spanish_guitar']=treated
    for name,definition in q['additional_definitions'].items():grouped[name],_=filter_layer(grouped['band'],definition)
    models={};plans={}
    for kind,parts in grouped.items():
        if not parts:
            write(kind+'-omitted.json',dict(reason='No source-supported instruments for this layer',note_count=0));continue
        folder=Path(kind);observations=[];native_parts=[];programs=[];aid=q['artifact_id']+'-'+kind+'-measured-analysis';rate=source['media']['timebase']['sample_rate_hz']
        provenance=lambda oid,reason:dict(state='inferred',evidence_refs=[dict(analysis_id=aid,observation_id=oid)],rationale=reason,correction_ref=None)
        observations.append(dict(id='display-clock',analyzer_id='muscriptor-recording',source=dict(start=0,end=source['media']['timebase']['samples_per_channel']),confidence_millionths=0,uncertainty='Display timebase only; musical tempo and meter are not inferred here',value=dict(kind='tempo',milli_bpm=60000)))
        for pi,part in enumerate(parts):
            notes=[];voice=10 if part['drums'] else pi+1
            if not part['drums'] and voice>=10:voice+=1
            if voice>16:raise ValueError('Source-supported ensemble exceeds MIDI channels')
            for ni,n in enumerate(part['notes']):
                start=round(n['start']*1000);end=round(n['end']*1000)
                if not 0<=start<end<=round(model_duration*1000):raise ValueError('Arrangement treatment exceeds declared source/postroll extent')
                oid=f'pitch-{pi:02}-{ni:05}';nid=kind+'-'+oid
                observations.append(dict(id=oid,analyzer_id='muscriptor-recording',source=dict(start=round(n['start']*rate),end=min(source['media']['timebase']['samples_per_channel'],round(n['end']*rate))),confidence_millionths=0,uncertainty='Uncalibrated model note candidate; no numeric confidence measured; arrangement treatment is explicit where applicable',value=dict(kind='pitch',midi_note=n['pitch'],cents=0)))
                notes.append(dict(id=nid,voice=voice,start_tick=start,duration_ticks=end-start,midi_note=n['pitch'],velocity=n['velocity'],provenance=provenance(oid,'Recording-derived MuScriptor note; explicit treatment ledger for Spanish guitar and hand-percussion colors')))
            notes=notation_voices(notes);native_parts.append(dict(id=part['id'],role='rhythm' if part['drums'] else 'vocal' if kind=='voice' else 'harmony',name=part['name'],notes=notes));programs.append(dict(id=part['id'],program=part['program'],drums=part['drums'],name=part['name'],source_kind=part['source_kind']))
        source_kind=parts[0]['source_kind'];weights=q['weights_by_kind'][source_kind]
        analysis=dict(schema='reel.music-analysis.v0.1',analysis_id=aid,source=binding,analyzers=[dict(id='muscriptor-recording',adapter='existing-muscriptor-recording-derived',version=q['muscriptor_version'],model_revision=weights['sha256'],parameters_sha256=descriptor('layer-request.json')['sha256'],license='Installed model and source library licenses apply',network_policy='denied')],stems=[],observations=observations,limitations=['Machine transcription requires musical review.','60 BPM and 4/4 are display placeholders, not source tempo/meter findings.','Unsupported source instruments are omitted.'],review=q['review'])
        # All source/analysis bindings are relative to the root job directory.
        analysis_name=kind+'-analysis.json';write(analysis_name,analysis);ar=native(q,'music-analysis-validate',analysis_name)
        auth=dict(source['authority'],status='draft-not-reviewed',decision_refs=[])
        lyrics=[]
        if q.get('canonical'):
            checked('lyrics.txt',q['canonical']);lyrics=[dict(id='canonical-poem',kind='canonical',language='es',path='lyrics.txt',sha256=q['canonical']['sha256'],authority=auth)]
        elif q.get('source_mode') not in ['instrumental','wordless-coro']:raise ValueError('A sung source requires its canonical text')
        model=dict(schema='reel.music-model.v0.1',model_id=q['artifact_id']+'-'+kind,source=binding,analyses=[dict(manifest=analysis_name,manifest_sha256=ar['manifest_sha256'],contract_sha256=ar['contract_sha256'],analysis_id=aid)],authority=auth,musical_timebase=timebase,duration_ticks=round(model_duration*1000),tempo_map=[dict(tick=0,microseconds_per_quarter=1000000,provenance=provenance('display-clock','Millisecond source clock; not an inferred musical tempo'))],meter_map=[dict(tick=0,numerator=4,denominator=4,provenance=provenance('display-clock','Unreviewed display placeholder'))],form=[dict(id='full-source',label='Complete selected full recording',range=dict(start=0,end=round(duration*1000)),provenance=provenance('display-clock','Exact complete source extent')),dict(id='postroll',label='Established edition postroll',range=dict(start=round(duration*1000),end=round(model_duration*1000)),provenance=provenance('display-clock','Explicit two-second edition postroll; original PCM prefix preserved'))],parts=native_parts,harmony=[],rhythm_cells=[],hooks=[],lyric_layers=lyrics,lead_sheet=None,expressive_timing=[],unknowns=analysis['limitations'],review=q['review'])
        model_name=kind+'-model.json';write(model_name,model);mr=native(q,'music-model-validate',model_name);plan_name=kind+'-score-plan.json';native(q,'music-score-export-plan',model_name,'--output-path',plan_name);export_dir=Path('native-export-'+kind);native(q,'music-score-export-render',plan_name,model_name,'--output-dir',str(export_dir));er=native(q,'music-score-export-check',str(export_dir/'receipt.json'),plan_name,model_name,str(export_dir))
        folder.mkdir(exist_ok=True)
        for filename in ['score.mid','score.musicxml','receipt.json']:
            (folder/filename).hardlink_to(export_dir/filename)
        checks=dict(source=sr,analysis=ar,model=mr,score_export=er);checks.update(arrangement_plan(q,model,model_name,mr,programs,kind));write(kind+'-programs.json',programs);write(kind+'-checks.json',checks);models[kind]=model_name;plans[kind]=plan_name
    write('guitar-treatment-ledger.json',ledgers);write('layer-catalog.json',dict(schema='reel.recording-derived-arrangement-layers.v1',golden_lock=q['golden_lock'],source=q['recording'],vocal_estimate=q['vocal_estimate'],models=models,score_plans=plans,source_note_counts={k:sum(len(p['notes']) for p in parts) for k,parts in grouped.items()},unsupported_layers=[k for k,p in grouped.items() if not p],lyrics_changed=False,musical_approval=False,source_transcription_evidence=q['transcription_evidence']))
    print('RECORDING-DERIVED NATIVE LAYERS VERIFIED',len(models),flush=True)


def main():
    offline();q=read('layer-request.json')
    if q['schema']!='reel.recording-derived-arrangement-layers-request.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Explicit private source-derived layer request required')
    build(q)
    # One bounded native output retains the complete layer family. Consumers
    # address exact archive members; this avoids exceeding the job slot limit.
    with zipfile.ZipFile('layer-payloads.zip','w',compression=zipfile.ZIP_DEFLATED) as archive:
        for kind in ['piano','guitar','band','voice','spanish_guitar',*q['additional_definitions']]:
            for suffix in ['analysis.json','model.json','score-plan.json','programs.json','checks.json','draft.json','arrangement-plan.json','omitted.json']:
                path=Path(kind+'-'+suffix)
                if path.exists():archive.write(path)
            for directory in [Path(kind),Path('native-export-'+kind)]:
                if directory.exists():
                    for path in sorted(directory.iterdir()):
                        if path.is_file():archive.write(path)


if __name__=='__main__':main()
