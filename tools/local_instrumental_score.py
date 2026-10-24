"""Source-clock instrumental studies through existing native REEL music contracts.

Measured notes, proposed pulse/meter and an explicitly selected lead family stay
distinct. No expected notes/lyrics, missing durations or automatic musical approval.
"""
import collections
from pathlib import Path
import subprocess
from local_song_backend import checked, descriptor, offline, read, write
from local_song_recording_layers import notation_voices
from local_song_piano_arrangement import native
from local_song_transcribe import ROLES


def note_parts(data, duration, bpm, lead_instrument=None, excluded_instruments=()):
    if data.get('validation_passed') is not True:
        raise ValueError('Invalid raw capture cannot enter an instrumental score')
    if not 20 <= bpm <= 300 or duration <= 0:
        raise ValueError('Unusable pulse or source extent')
    usq = round(60e6 / bpm)
    tick = lambda second: round(second * 1e6 / usq * 960)
    groups = collections.defaultdict(list)
    identities = set()
    names={n['instrument'] for n in data['notes']}
    if not set(excluded_instruments)<=names:
        raise ValueError('Excluded source family is absent from measured evidence')
    for note in data['notes']:
        if note['id'] in identities:
            raise ValueError('Duplicate source note identity')
        identities.add(note['id'])
        if note['instrument'] in excluded_instruments:
            continue
        if not 0 <= note['start'] < note['end'] <= duration:
            raise ValueError('Source note span exceeds complete recording')
        if not isinstance(note['pitch'], int) or not 0 <= note['pitch'] <= 127:
            raise ValueError('Invalid source pitch')
        start, end = tick(note['start']), tick(note['end'])
        if start >= end:
            raise ValueError('Measured note collapses at the declared score clock')
        groups[note['instrument']].append(dict(source_id=note['id'],
            start_tick=start, duration_ticks=end-start, midi_note=note['pitch'],
            velocity=80, source_start=note['start'], source_end=note['end']))
    if not groups or lead_instrument is not None and lead_instrument not in groups:
        raise ValueError('Explicit score lead is absent from measured source')
    if len(groups) > 15:
        raise ValueError('Instrumental study exceeds qualified MIDI part capacity')
    parts=[]
    for i, (name, notes) in enumerate(sorted(groups.items())):
        if name not in data['programs']:
            raise ValueError('Measured instrument has no declared program')
        parts.append(dict(id='source-part-'+str(i), instrument=name,
            program=data['programs'][name], drums=name=='drums',
            role='melody' if name==lead_instrument else 'rhythm' if name=='drums' else 'harmony',
            notes=notes))
    return parts, usq, tick(duration)


def verify_source_clock(parts, data, usq, tolerance=.001, excluded_instruments=()):
    expected={n['id']:n for n in data['notes'] if n['instrument'] not in excluded_instruments};actual={}
    for part in parts:
        for n in part['notes']:
            start=n['start_tick']*usq/960e6
            end=(n['start_tick']+n['duration_ticks'])*usq/960e6
            source=expected[n['source_id']]
            if n['source_id'] in actual or n['midi_note']!=source['pitch']:
                raise ValueError('Instrumental projection changed note identity or pitch')
            if max(abs(start-source['start']),abs(end-source['end'])) > tolerance:
                raise ValueError('Instrumental score clock differs from measured source')
            actual[n['source_id']]=dict(start=start,end=end,pitch=n['midi_note'])
    if actual.keys()!=expected.keys():
        raise ValueError('Instrumental projection dropped measured notes')
    return dict(notes=len(actual),maximum_allowed_seconds=tolerance,
                every_selected_note_and_pitch_preserved=True,source_clock_verified=True,
                omitted_source_events=len(data['notes'])-len(actual),excluded_instruments=list(excluded_instruments))


def main():
    offline();q=read('instrumental-score-request.json')
    if q['schema']!='reel.instrumental-score-request.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):
        raise ValueError('Explicit private offline instrumental score request required')
    checked('original.wav',q['recording']);checked('notes.json',q['measured_notes'])
    checked('reel.exe',q['native_runtime']);ff=checked(q['ffmpeg']['path'],q['ffmpeg'])
    data=read('notes.json')
    if data['source']!=q['recording']:
        raise ValueError('Measured notes refer to a different recording')
    excluded=q.get('excluded_instruments',[])
    if excluded and not q.get('exclusion_rationale'):
        raise ValueError('Explicit source-part exclusion needs a retained rationale')
    parts,usq,total=note_parts(data,q['duration_seconds'],q['tempo_hypothesis_bpm'],q.get('lead_instrument'),excluded)
    clock=verify_source_clock(parts,data,usq,excluded_instruments=excluded)
    rate=16000;raw=subprocess.check_output([str(ff),'-v','error','-nostdin','-i','original.wav','-ac','1','-ar',str(rate),'-f','f32le','-'])
    samples=len(raw)//4
    if len(raw)%4 or abs(samples/rate-q['duration_seconds'])>1/rate:
        raise ValueError('Decoded PCM differs from complete Golden extent')
    Path('source.pcm').write_bytes(raw);pcm=descriptor('source.pcm')
    authority=dict(namespace='private-instrumental-score',artifact_id=q['artifact_id'],content_sha256=q['authority']['sha256'],status='draft-not-reviewed',required_roles=ROLES,decision_refs=[])
    timebase=dict(pulses_per_quarter=960,rounding='half-away-from-zero')
    source=dict(schema='reel.music-source.v0.1',source_id=q['artifact_id'],media=dict(path='source.pcm',sha256=pcm['sha256'],format='raw-pcm-f32le',timebase=dict(sample_rate_hz=rate,channels=1,samples_per_channel=samples),decoded_pcm_sha256=pcm['sha256']),musical_timebase=timebase,authority=authority,egress=dict(private=True,network_policy='denied',third_party_upload=False))
    write('source.json',source);sr=native(q,'music-source-validate','source.json')
    binding=dict(manifest='source.json',manifest_sha256=sr['manifest_sha256'],contract_sha256=sr['contract_sha256'],decoded_pcm_sha256=pcm['sha256'])
    aid=q['artifact_id']+'-measured-analysis'
    provenance=lambda oid,reason:dict(state='inferred',evidence_refs=[dict(analysis_id=aid,observation_id=oid)],rationale=reason,correction_ref=None)
    observations=[dict(id='pulse-hypothesis',analyzer_id='measured-instrumental',source=dict(start=0,end=samples),confidence_millionths=0,uncertainty='Proposed display pulse; meter and beat phase require musical review',value=dict(kind='tempo',milli_bpm=round(q['tempo_hypothesis_bpm']*1000)))]
    native_parts=[];programs=[];ledger=[]
    for part in parts:
        notes=[]
        for n in part['notes']:
            oid='measured-'+str(n['source_id']);nid=part['id']+'-'+oid
            observations.append(dict(id=oid,analyzer_id='measured-instrumental',source=dict(start=round(n['source_start']*rate),end=min(samples,round(n['source_end']*rate))),confidence_millionths=0,uncertainty='Model note hypothesis; confidence not calibrated; source instrument label unconfirmed',value=dict(kind='pitch',midi_note=n['midi_note'],cents=0)))
            notes.append(dict(id=nid,voice=1,start_tick=n['start_tick'],duration_ticks=n['duration_ticks'],midi_note=n['midi_note'],velocity=n['velocity'],provenance=provenance(oid,'Preserve measured note pitch and interval; musical accuracy unreviewed')))
            ledger.append(dict(source_note_id=n['source_id'],native_note_id=nid,source_instrument=part['instrument'],part=part['id']))
        notes=notation_voices(notes)
        native_parts.append(dict(id=part['id'],role=part['role'],name=part['instrument'].replace('_',' ')+' (unreviewed)',notes=notes))
        programs.append(dict(id=part['id'],program=part['program'],drums=part['drums'],name=part['instrument'],source_kind='unconstrained'))
    limitations=['Machine polyphonic note hypotheses require musical review.','Instrument labels and velocities are unconfirmed; velocity 80 is a diagnostic rendering choice.','Pulse, meter, key, harmony and musical form are not approved.','No words or syllable alignment are inferred.','The complete source-clock ensemble study is distinct from a corrected playable lead sheet.']
    analysis=dict(schema='reel.music-analysis.v0.1',analysis_id=aid,source=binding,analyzers=[dict(id='measured-instrumental',adapter='offline-muscriptor-measured-notes',version='1',model_revision=q['weights']['sha256'],parameters_sha256=descriptor('instrumental-score-request.json')['sha256'],license='Original installed model and tool licenses apply',network_policy='denied')],stems=[],observations=observations,limitations=limitations,review=dict(status='not-reviewed',required_roles=ROLES,decision_refs=[]))
    write('analysis.json',analysis);ar=native(q,'music-analysis-validate','analysis.json')
    lead=next((p for p in native_parts if p['role']=='melody'),None)
    model=dict(schema='reel.music-model.v0.1',model_id=q['artifact_id'],source=binding,analyses=[dict(manifest='analysis.json',manifest_sha256=ar['manifest_sha256'],contract_sha256=ar['contract_sha256'],analysis_id=aid)],authority=authority,musical_timebase=timebase,duration_ticks=total,tempo_map=[dict(tick=0,microseconds_per_quarter=usq,provenance=provenance('pulse-hypothesis','Declared provisional score pulse; exact measured note seconds preserved'))],meter_map=[dict(tick=0,numerator=q['display_meter'][0],denominator=q['display_meter'][1],provenance=provenance('pulse-hypothesis','Unreviewed display meter; no source meter approval inferred'))],form=[dict(id='full-recording',label='Full source; musical sections pending',range=dict(start=0,end=total),provenance=provenance('pulse-hypothesis','Complete Golden extent, not a form transcription'))],parts=native_parts,harmony=[],rhythm_cells=[],hooks=[],lyric_layers=[],lead_sheet=dict(title=q['title']+' | Instrumental working study',melody_part_id=lead['id'],lyric_layer_id=None,underlay=[]) if lead else None,expressive_timing=[],unknowns=limitations,review=dict(status='not-reviewed',required_roles=ROLES,decision_refs=[]))
    write('model.json',model);mr=native(q,'music-model-validate','model.json');native(q,'music-score-export-plan','model.json','--output-path','score-plan.json');native(q,'music-score-export-render','score-plan.json','model.json','--output-dir','rendered');er=native(q,'music-score-export-check','rendered/receipt.json','score-plan.json','model.json','rendered')
    write('programs.json',programs);write('note-ledger.json',dict(clock=clock,notes=ledger,omitted=[dict(n,reason=q.get('exclusion_rationale')) for n in data['notes'] if n['instrument'] in excluded],source_golden=q['golden_lock'],wordless_coro=q.get('wordless_coro',False),human_review=False));write('native-checks.json',dict(source=sr,analysis=ar,model=mr,score_export=er))
    # The existing generic draft/plan helper expects a display-clock observation.
    # The validated score packet is authoritative here; arrangement dispositions
    # are authored later against the reviewed rhythmic/form model.
    for name in ['score.mid','score.musicxml','rehearsal-guide.wav','receipt.json']+(['lead-sheet.svg'] if lead else []):
        (Path(name)).hardlink_to(Path('rendered')/name)
    write('study-summary.json',dict(source_golden=q['golden_lock'],note_count=sum(len(p['notes']) for p in parts),parts=[dict(instrument=p['instrument'],notes=len(p['notes'])) for p in parts],lead_instrument=q.get('lead_instrument'),tempo_hypothesis_bpm=q['tempo_hypothesis_bpm'],display_meter=q['display_meter'],seconds=q['duration_seconds'],source_clock=clock,source_unchanged=True,lyrics_supplied=False,musical_approval=False,limitations=limitations))
    print('NATIVE INSTRUMENTAL SOURCE STUDY VERIFIED',clock['notes'],flush=True)


if __name__=='__main__':main()
