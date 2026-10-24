"""Measured, gridded instrumental reading scores with reversible timing ledgers.

The captured source model remains immutable. This authors a separately labelled
notation proposal and preserves captured timing in native expressive offsets.
"""
import copy
import math
from pathlib import Path
from local_song_backend import checked,read,write,offline,descriptor
from local_song_piano_arrangement import native


def nearest(value):
    return math.floor(value+.5) if value>=0 else math.ceil(value-.5)


def reading_notes(notes,old_usq,new_usq,ppq,duration,phase,division=8):
    if ppq%division or division not in [4,8,12,24]:raise ValueError('Unsupported explicit notation grid')
    grid=ppq//division;result=[];ledger=[]
    for old in notes:
        if old['duration_ticks']<=0:raise ValueError('A missing source duration cannot be manufactured')
        exact_start=nearest(old['start_tick']*old_usq/new_usq);exact_end=nearest((old['start_tick']+old['duration_ticks'])*old_usq/new_usq)
        if exact_start<0 or exact_end>duration or exact_end<=exact_start:raise ValueError('Captured source extent differs from notation clock')
        start=max(0,min(duration-1,phase+nearest((exact_start-phase)/grid)*grid))
        end=min(duration,max(start+1,phase+nearest((exact_end-phase)/grid)*grid))
        if end==start+1 and exact_end-exact_start<grid:end=min(duration,start+grid)
        note=copy.deepcopy(old);note.update(start_tick=start,duration_ticks=end-start)
        row=dict(note_id=old['id'],source_start_tick=old['start_tick'],source_duration_ticks=old['duration_ticks'],source_tempo_usq=old_usq,notation_start_tick=start,notation_duration_ticks=end-start,onset_offset_ticks=exact_start-start,duration_offset_ticks=(exact_end-exact_start)-(end-start),midi_note=old['midi_note'])
        if note['start_tick']+row['onset_offset_ticks']!=exact_start or note['duration_ticks']+row['duration_offset_ticks']!=exact_end-exact_start:raise ValueError('Expressive offsets lost source clock')
        result.append(note);ledger.append(row)
    return result,ledger


def chord_voices(notes,maximum=4):
    groups={}
    for note in notes:groups.setdefault((note['start_tick'],note['duration_ticks']),[]).append(note)
    ends=[];out=[]
    for (start,length),group in sorted(groups.items()):
        # Quantization can put distinct captured unisons on one nominal chord.
        # Retain every event on an independent voice rather than dropping it.
        lanes=[]
        for note in group:
            lane=next((lane for lane in lanes if all(n['midi_note']!=note['midi_note'] for n in lane)),None)
            if lane is None:lane=[];lanes.append(lane)
            lane.append(note)
        for lane in lanes:
            voice=next((i for i,end in enumerate(ends) if end<=start),len(ends))
            if voice==len(ends):
                if len(ends)>=maximum:raise ValueError('Reading score exceeds independent voice capacity')
                ends.append(0)
            ends[voice]=start+length
            out.extend(dict(note,voice=voice+1) for note in lane)
    return sorted(out,key=lambda n:(n['start_tick'],n['voice'],n['midi_note']))


def grand_voice_groups(notes):
    colored=chord_voices(notes,8);voices=sorted({n['voice'] for n in colored},key=lambda voice:sum(n['midi_note'] for n in colored if n['voice']==voice)/sum(n['voice']==voice for n in colored));lower=set(voices[:len(voices)//2]);upper=set(voices)-lower
    if not lower or not upper or max(len(lower),len(upper))>4:raise ValueError('Source cannot fit two four-voice reading staves')
    def staff(group):
        mapping={v:i+1 for i,v in enumerate(sorted(group))}
        return [dict(n,voice=mapping[n['voice']]) for n in colored if n['voice'] in group]
    return staff(upper),staff(lower)


def main():
    offline();q=read('notation-request.json')
    if q['schema']!='reel.instrumental-reading-score-request.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Explicit private reading-score request required')
    for name,key in [('parent-model.json','parent_model'),('form-report.json','form_report'),('source.json','source_manifest'),('source.pcm','pcm')]:checked(name,q[key])
    checked('reel.exe',q['native_runtime']);parent=read('parent-model.json');report=read('form-report.json');source=read('source.json')
    postroll=q.get('postroll_seconds',0)
    if postroll not in [0,2] or report['source']['sha256']!=q['recording']['sha256'] or abs(report['seconds']-(q['seconds']-postroll))>.002:raise ValueError('Acoustic analysis refers to another full recording or undeclared postroll')
    if len(parent['tempo_map'])!=1:raise ValueError('Captured source study has an unsupported variable display clock')
    ppq=parent['musical_timebase']['pulses_per_quarter'];old_usq=parent['tempo_map'][0]['microseconds_per_quarter'];bpm=report['tempo']['selected_quarter_reference_bpm'];usq=nearest(60e6/bpm);tick=lambda s:nearest(s*1e6/usq*ppq);duration=tick(q['seconds']);beats=report['tempo']['beat_seconds'];phase=tick(beats[0]) if beats else 0
    meter=report['display_meter'];bar=ppq*4*meter[0]//meter[1];phase=(nearest(phase/(ppq/8))*(ppq//8))%bar
    aid=q['artifact_id']+'-notation-analysis';rate=source['media']['timebase']['sample_rate_hz'];frames=source['media']['timebase']['samples_per_channel']
    obs=[dict(id='notation-grid',analyzer_id='source-form',source=dict(start=0,end=frames),confidence_millionths=0,uncertainty='Explicit 1/32-note reading grid and provisional beat phase; original timing retained in offsets',value=dict(kind='rhythm',label='Producer notation projection')),dict(id='pulse',analyzer_id='source-form',source=dict(start=0,end=frames),confidence_millionths=0,uncertainty='Recording-based reference pulse; half/double-time alternatives retained',value=dict(kind='tempo',milli_bpm=nearest(bpm*1000)))]
    def provenance(oid,reason):return dict(state='inferred',evidence_refs=[dict(analysis_id=aid,observation_id=oid)],rationale=reason,correction_ref=None)
    def add(oid,a,b,value):obs.append(dict(id=oid,analyzer_id='source-form',source=dict(start=max(0,round(a*rate)),end=min(frames,max(1,round(b*rate)))),confidence_millionths=0,uncertainty='Source-derived unreviewed proposal',value=value))
    model=copy.deepcopy(parent);model.update(model_id=q['artifact_id'],duration_ticks=duration,tempo_map=[dict(tick=0,microseconds_per_quarter=usq,provenance=provenance('pulse','Reference pulse from the full recording; alternate interpretations retained'))],meter_map=[dict(tick=0,numerator=meter[0],denominator=meter[1],provenance=provenance('notation-grid','Producer display meter and beat phase require musical review'))],parts=[],form=[],harmony=[],rhythm_cells=[],hooks=[],lead_sheet=None,expressive_timing=[])
    model.pop('piano_vocal_score',None);model.pop('instrumental_score',None);model['authority']=dict(model['authority'],artifact_id=q['artifact_id'],content_sha256=q['authority']['sha256'])
    ledger=[];groups=[]
    for part in parent['parts']:
        notes,rows=reading_notes(part['notes'],old_usq,usq,ppq,duration,phase);ledger.extend(rows)
        for n in notes:n['provenance']['evidence_refs'].append(dict(analysis_id=aid,observation_id='notation-grid'));n['provenance']['state']='inferred';n['provenance']['rationale']='Source pitch retained; nominal reading rhythm proposed with reversible captured-clock offsets'
        piano='piano' in part['name'].lower()
        try:single=chord_voices(notes);dense=False
        except ValueError:single=None;dense=True
        if part['role']=='rhythm' and dense:
            colored=chord_voices(notes,32)
            for block in range((max(n['voice'] for n in colored)+3)//4):
                pid=part['id']+'-percussion-'+str(block+1);subset=[dict(n,voice=n['voice']-4*block) for n in colored if 4*block<n['voice']<=4*(block+1)]
                model['parts'].append(dict(part,id=pid,name=part['name']+' '+str(block+1),notes=subset));groups.append(dict(id=pid,name=part['name']+' '+str(block+1),staff_part_ids=[pid],clef='percussion'))
            continue
        cuts=[]
        if piano or dense:
            for cut in sorted(range(48,77),key=lambda x:abs(x-60)):
                high=[n for n in notes if n['midi_note']>=cut];low=[n for n in notes if n['midi_note']<cut]
                if not high or not low:continue
                try:upper_notes=chord_voices(high);lower_notes=chord_voices(low)
                except ValueError:continue
                cuts.append((cut,upper_notes,lower_notes));break
            if not cuts and dense:
                upper_notes,lower_notes=grand_voice_groups(notes);cuts.append((None,upper_notes,lower_notes))
        if cuts:
            cut,upper_notes,lower_notes=cuts[0]
            upper=dict(part,id=part['id']+'-upper',name=part['name']+' upper',notes=upper_notes);lower=dict(part,id=part['id']+'-lower',name=part['name']+' lower',role='bass',notes=lower_notes);model['parts'].extend([upper,lower]);ids=[upper['id'],lower['id']];clef='grand'
        else:
            if single is None:raise ValueError('No readable four-voice staff grouping for '+part['name'])
            p=dict(part,notes=single);model['parts'].append(p);ids=[p['id']];clef='percussion' if part['role']=='rhythm' else 'bass' if sum(n['midi_note'] for n in notes)/len(notes)<55 else 'treble'
        groups.append(dict(id=part['id'],name=part['name'],staff_part_ids=ids,clef=clef))
    for row in ledger:
        if row['onset_offset_ticks'] or row['duration_offset_ticks']:model['expressive_timing'].append(dict(note_id=row['note_id'],onset_offset_ticks=row['onset_offset_ticks'],duration_offset_ticks=row['duration_offset_ticks'],provenance=provenance('notation-grid','Restore the captured source interval relative to the nominal reading grid')))
    for i,b in enumerate(report['bars']):
        a,z=tick(b['start']),tick(b['end'])
        if a>=z:continue
        oid='harmony-'+str(i);add(oid,b['start'],b['end'],dict(kind='harmony',symbol=b['symbol']));model['harmony'].append(dict(id=oid,range=dict(start=a,end=z),symbol=b['symbol'],provenance=provenance(oid,'Unreviewed triad reduction; inversions and extensions unresolved')))
    for i,s in enumerate(report['sections']):
        oid='section-'+str(i);add(oid,s['start'],s['end'],dict(kind='form',label=s['label']));model['form'].append(dict(id=oid,label=s['label'],range=dict(start=tick(s['start']),end=tick(s['end'])),provenance=provenance(oid,'Acoustic change proposal; musical function unresolved')))
    if postroll:
        oid='declared-postroll';add(oid,report['seconds'],q['seconds'],dict(kind='form',label='Declared edition postroll'));model['form'].append(dict(id=oid,label='Declared edition postroll',range=dict(start=tick(report['seconds']),end=duration),provenance=provenance(oid,'Explicit source-layer postroll; not added performance content')))
    analysis=dict(schema='reel.music-analysis.v0.1',analysis_id=aid,source=parent['source'],analyzers=[dict(id='source-form',adapter='source-form-and-explicit-reading-grid',version='1',model_revision='no-pretrained-harmony-model',parameters_sha256=descriptor('notation-request.json')['sha256'],license='Original numeric dependency licenses apply',network_policy='denied')],stems=[],observations=obs,limitations=report['limitations']+['The nominal score grid is a proposal; raw source timing stays in expressive offsets and original source-study exports.'],review=parent['review'])
    write('notation-analysis.json',analysis);ar=native(q,'music-analysis-validate','notation-analysis.json');model['analyses'].append(dict(manifest='notation-analysis.json',manifest_sha256=ar['manifest_sha256'],contract_sha256=ar['contract_sha256'],analysis_id=aid));model['instrumental_score']=dict(title=q['title'],parts=groups,pickup_ticks=phase,key_fifths=None);model['unknowns']=list(dict.fromkeys(model['unknowns']+analysis['limitations']+['Tonal-center candidates are not an approved key signature.','Instrument identities, readable rhythm, voicing and section function need musical review.']))
    write('reading-model.json',model);mr=native(q,'music-model-validate','reading-model.json');native(q,'music-score-export-plan','reading-model.json','--output-path','reading-plan.json');native(q,'music-score-export-render','reading-plan.json','reading-model.json','--output-dir','rendered');er=native(q,'music-score-export-check','rendered/receipt.json','reading-plan.json','reading-model.json','rendered');write('native-checks.json',dict(analysis=ar,model=mr,export=er));write('timing-ledger.json',dict(source_golden=q['golden_lock'],parent_model=q['parent_model'],form_report=q['form_report'],reading_grid='1/32 note',source_notes=len(ledger),note_rows=ledger,pitches_preserved=True,raw_source_model_unchanged=True,musical_approval=False))
    for name in ['score.mid','score.musicxml','rehearsal-guide.wav','instrumental-score.musicxml','receipt.json']:Path(name).hardlink_to(Path('rendered')/name)
    print('MEASURED INSTRUMENTAL READING SCORE VERIFIED',len(ledger),'source notes',flush=True)


if __name__=='__main__':main()
