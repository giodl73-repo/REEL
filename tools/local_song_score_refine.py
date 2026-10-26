"""Refine existing lyric studies from retained acoustic frames, without new F0.

Only previously unmapped syllables are eligible. The original high-confidence
voicing threshold is retained and original-mix agreement is required for every
added frame. Short stable runs are explicit unreviewed notation proposals.
"""
import copy
import math
import os
from pathlib import Path
from local_song_backend import checked, offline, read, write
from local_song_transcribe import native, ROLES


def supported_runs(f0,valid,mix_f0,probability,hop,start,end,minimum=.04):
    import numpy as np
    first=max(0,math.ceil(start/hop));last=min(len(f0),math.floor(end/hop))
    if last<=first:return []
    f=np.asarray(f0)[first:last];m=np.asarray(mix_f0)[first:last];p=np.asarray(probability)[first:last]
    eligible=np.asarray(valid)[first:last] & np.isfinite(f) & np.isfinite(m) & (f>0) & (m>0) & (p>=.65)
    cents=np.full(len(f),np.inf);cents[eligible]=np.abs(1200*np.log2(m[eligible]/f[eligible]));eligible &= cents<=75
    pitches=np.full(len(f),-1,dtype=int);pitches[eligible]=np.rint(69+12*np.log2(f[eligible]/440)).astype(int)
    changes=np.r_[0,np.flatnonzero(np.diff(pitches))+1,len(pitches)];runs=[]
    for a,b in zip(changes[:-1],changes[1:]):
        if pitches[a]<0 or (b-a)*hop+1e-9<minimum:continue
        runs.append(dict(start=(first+int(a))*hop,end=(first+int(b))*hop,midi_note=int(pitches[a]),confidence=float(np.median(p[a:b])),frames=int(b-a),maximum_mix_difference_cents=float(cents[a:b].max())))
    return runs


def refine(parent,evidence,frames):
    import numpy as np
    model=copy.deepcopy(parent);ev=copy.deepcopy(evidence);hop=float(frames['hop'])/float(frames['sample_rate']);scale=parent['tempo_map'][0]['microseconds_per_quarter']/1e6/parent['musical_timebase']['pulses_per_quarter']
    notes=model['parts'][0]['notes'];underlay=model['lead_sheet']['underlay'];added=[];rejected=[]
    for index,row in enumerate(ev['syllables']):
        if row['note_ids']:continue
        if row['line'] in ev['owner_accepted_omissions']:raise ValueError('Study proposes an accepted omission')
        if row['mean_log_probability'] < -4:
            rejected.append(dict(syllable_index=index,reason='low constrained-character acoustic support'));continue
        runs=supported_runs(frames['f0'],frames['valid'],frames['mix_f0'],frames['probability'],hop,*row['span_seconds'])
        ids=[]
        for run in runs:
            start=math.ceil(run['start']/scale);end=math.floor(run['end']/scale)
            if end<=start:continue
            if any(start<n['start_tick']+n['duration_ticks'] and end>n['start_tick'] for n in notes):raise ValueError('Refinement overlaps retained or added notation')
            oid=f'frame-run-{len(added):04}';nid=f'refined-note-{len(added):04}'
            notes.append(dict(id=nid,voice=1,start_tick=start,duration_ticks=end-start,midi_note=run['midi_note'],velocity=72,provenance=dict(state='inferred',evidence_refs=[dict(analysis_id=parent['model_id']+'-frame-refinement',observation_id=oid)],rationale='Previously unmapped syllable intersects a retained >=40ms stable voiced run; every frame agrees with original-mix F0 within 75 cents; musical review pending',correction_ref=None)))
            ids.append(nid);added.append(dict(observation_id=oid,note_id=nid,syllable_index=index,line=row['line'],text=row['text'],**run))
        if ids:
            row['note_ids']=ids;row['status']='retained-frame short-run pitch proposal; musical review pending'
            underlay.append(dict(id=f'refined-syllable-{index:04}',note_ids=ids,text_start_byte=row['text_start_byte'],text_end_byte=row['text_end_byte'],syllabic=row['syllabic']))
        else:rejected.append(dict(syllable_index=index,reason='No >=40ms contiguous same-pitch high-confidence original-mix agreement'))
    notes.sort(key=lambda n:(n['start_tick'],n['voice'],n['id']));underlay.sort(key=lambda u:next(n['start_tick'] for n in notes if n['id']==u['note_ids'][0]))
    model['model_id']+='-retained-frame-refinement-v1'
    ev.update(refinement=dict(added=added,rejected=rejected,minimum_stable_run_seconds=.04,voiced_probability_threshold=.65,maximum_original_mix_difference_cents=75,existing_notes_unchanged=True,independent_word_presence_test=False),finished_score=False)
    return model,ev,added


def main():
    offline();q=read('refine.json')
    if q['schema']!='reel.song-score-refinement.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Private source-bound refinement request required')
    for name,key in [('parent-model.json','model'),('parent-evidence.json','evidence'),('pitch-frames.npz','frames'),('pitch-evidence.json','pitch_evidence'),('expected-singing.txt','expected_singing')]:checked(name,q[key])
    checked('reel.exe',q['native_runtime'])
    for name,version in q['runtime_packages'].items():
        if __import__('importlib.metadata',fromlist=['version']).version(name)!=version:raise ValueError('Runtime package changed: '+name)
    import numpy as np
    parent=read('parent-model.json');evidence=read('parent-evidence.json');pitch=read('pitch-evidence.json')
    if pitch['source']!=evidence['request']['recording'] or pitch['vocal_estimate']!=evidence['request']['vocal_estimate']:raise ValueError('Acoustic frames belong to a different recording or vocal estimate')
    # A frame archive is retained beside its exact original evidence; the request
    # pins both, plus the original full-Golden package lock.
    with np.load('pitch-frames.npz',allow_pickle=False) as frames:model,ev,added=refine(parent,evidence,frames)
    parameters=dict(minimum_stable_run_seconds=.04,voiced_probability_threshold=.65,original_mix_agreement_cents=75,retained_frame_archive=q['frames'],parent_model=q['model'],canonical_text_used_for_pitch=False)
    write('refinement-parameters.json',parameters)
    from local_song_backend import descriptor
    sr=read('source.json')['media']['timebase']['sample_rate_hz'];aid=parent['model_id']+'-frame-refinement'
    analysis=dict(schema='reel.music-analysis.v0.1',analysis_id=aid,source=parent['source'],analyzers=[dict(id='retained-frames',adapter='retained-pyin-original-yin-agreement',version='1',model_revision='no-new-model-or-estimation',parameters_sha256=descriptor('refinement-parameters.json')['sha256'],license='Original analysis/library licenses retained',network_policy='denied')],stems=[],observations=[dict(id=a['observation_id'],analyzer_id='retained-frames',source=dict(start=round(a['start']*sr),end=round(a['end']*sr)),confidence_millionths=round(a['confidence']*1e6),uncertainty='Short acoustic run and forced-text timing; unreviewed notation',value=dict(kind='pitch',midi_note=a['midi_note'],cents=0)) for a in added],limitations=['No new independently recognized words.','Short supported runs remain pitch/articulation proposals; unresolved gaps remain open.'],review=dict(status='not-reviewed',required_roles=ROLES,decision_refs=[]))
    if added:
        write('refinement-analysis.json',analysis);ar=native('music-analysis-validate','refinement-analysis.json')
        model['analyses'].append(dict(manifest='refinement-analysis.json',manifest_sha256=ar['manifest_sha256'],contract_sha256=ar['contract_sha256'],analysis_id=aid))
    else:
        # No supported run is a valid conservative result, not a music-analysis
        # document with an invalid empty observation list. Keep existing source
        # analyses and every note/underlay tuple, and export the unchanged study.
        ar=dict(skipped=True,reason='No additional supported pitch observations',existing_analyses_unchanged=True)
        write('refinement-analysis.json',dict(schema='reel.song-score-refinement-no-additions.v1',
              source=parent['source'],parent_model=q['model'],added_observations=0,
              retained_notes_unchanged=True,unsupported_gaps_retained=True))
    model['unknowns']+=analysis['limitations'];write('model.json',model);write('syllable-study.json',ev)
    checks=dict(analysis=ar,model=native('music-model-validate','model.json'))
    native('music-score-export-plan','model.json','--output-path','score-plan.json');native('music-score-export-render','score-plan.json','model.json','--output-dir','rendered');checks['export']=native('music-score-export-check','rendered/receipt.json','score-plan.json','model.json','rendered');write('native-checks.json',checks)
    for name in ['score.mid','score.musicxml','lead-sheet.svg','rehearsal-guide.wav','receipt.json']:os.link(Path('rendered')/name,name)
    print('REFINED',len(added),'supported short pitch runs; prior notes unchanged',flush=True)


if __name__=='__main__':main()
