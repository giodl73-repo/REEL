"""Explicit offline recording-derived MuScriptor passes for native music work.

This adapts an already installed model; it never downloads, starts a server or
fills missing note durations. Raw model events remain distinct from scores and
later arrangement treatments. No lyric text is supplied to the estimator.
"""
import collections
import dataclasses
import json
import math
from pathlib import Path
import time
from local_song_backend import checked, offline, read, write


def paired_notes(events,duration):
    starts={};ends={}
    for event in events:
        if event['type']=='start':
            if event['index'] in starts:raise ValueError('Duplicate transcription note start')
            starts[event['index']]=event
        elif event['type']=='end':
            if event['start_event_index'] in ends:raise ValueError('Duplicate transcription note end')
            ends[event['start_event_index']]=event['end_time']
    if starts.keys()!=ends.keys():raise ValueError('Unpaired transcription note events')
    notes=[];outside=[]
    for identifier,event in starts.items():
        start=event['start_time'];end=ends[identifier]
        if not all(isinstance(x,(int,float)) and math.isfinite(x) for x in [start,end]) or not 0<=start<end or not 0<=event['pitch']<=127:raise ValueError('Invalid measured note span; no manufactured duration')
        row=dict(id=identifier,start=start,end=end,pitch=event['pitch'],instrument=event['instrument'])
        if end>duration:outside.append(dict(**row,status='outside-full-recording; quarantined'))
        else:notes.append(row)
    return sorted(notes,key=lambda n:(n['start'],n['id'])),outside


def diagnose_events(events):
    starts={};ends={};issues=[]
    for event in events:
        if event['type']=='start':starts.setdefault(event['index'],[]).append(event)
        elif event['type']=='end':ends.setdefault(event['start_event_index'],[]).append(event['end_time'])
    for identifier in sorted(starts.keys()|ends.keys()):
        begin=starts.get(identifier,[]);finish=ends.get(identifier,[])
        if len(begin)!=1 or len(finish)!=1:issues.append(dict(id=identifier,reason='unpaired-or-duplicate-events',starts=len(begin),ends=len(finish)));continue
        start=begin[0]['start_time'];end=finish[0]
        if not all(isinstance(x,(int,float)) and math.isfinite(x) for x in [start,end]) or not 0<=start<end:issues.append(dict(id=identifier,reason='invalid-measured-span',start=start,end=end))
    return issues


def main():
    offline();q=read('transcribe-request.json')
    if q['schema']!='reel.recording-derived-muscriptor.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Explicit private offline transcription request required')
    checked('original.wav',q['recording']);checked('vocals.wav',q['vocal_estimate'])
    for row in q['runtime_files']:checked(row['path'],row)
    import importlib.metadata
    for name,version in q['runtime_packages'].items():
        if importlib.metadata.version(name)!=version:raise ValueError('Installed transcription package changed: '+name)
    import torch
    from muscriptor.transcription_model import TranscriptionModel
    from muscriptor.events import NoteStartEvent,NoteEndEvent,ProgressEvent
    torch.manual_seed(7301);torch.set_num_threads(2)
    model_path=checked(q['weights']['path'],q['weights']);checked(q['model_config']['path'],q['model_config'])
    if not torch.cuda.is_available():raise ValueError('Frozen CUDA transcription runtime unavailable')
    print('LOAD EXACT OFFLINE MUSCRIPTOR MODEL',flush=True);model=TranscriptionModel.load_model(weights_path=str(model_path),device='cuda',dtype='float32')
    passes=[];roster=q.get('band_roster');programs={}
    selected=q.get('passes',['unconstrained','piano','guitar','band','voice'])
    if not selected or len(set(selected))!=len(selected) or any(x not in ['unconstrained','piano','guitar','band','voice'] for x in selected):raise ValueError('Unknown or duplicate transcription pass')
    for kind,source,instruments in [('unconstrained','original.wav',None),('piano','original.wav',['acoustic_piano']),('guitar','original.wav',['acoustic_guitar']),('band','original.wav','detected'),('voice','vocals.wav',['voice'])]:
        if kind not in selected:continue
        if instruments=='detected':
            instruments=roster
            if not instruments:raise ValueError('No supported recording-derived band roster; no fabricated instruments')
        events=[];start=time.monotonic();progress=[];print('TRANSCRIBE',kind,instruments,flush=True)
        for event in model.transcribe(source,instruments=instruments,use_sampling=False,batch_size=1,prelude_forcing=True,no_eos_is_ok=True):
            if isinstance(event,NoteStartEvent):events.append(dict(type='start',**dataclasses.asdict(event)))
            elif isinstance(event,NoteEndEvent):events.append(dict(type='end',end_time=event.end_time,start_event_index=event.start_event_index))
            elif isinstance(event,ProgressEvent):
                progress.append(dict(completed=event.completed,total=event.total,elapsed_seconds=time.monotonic()-start));print('CHUNK',kind,event.completed,event.total,flush=True)
        # Raw capture succeeds independently of semantic note validation. The
        # coordinator must gate downstream use; failed measurements stay cached.
        write(kind+'-events.json',events);write(kind+'-progress.json',progress)
        validation_error=None
        try:notes,outside=paired_notes(events,q['duration_seconds'])
        except ValueError as error:notes=[];outside=[];validation_error=str(error)
        counts=collections.Counter(n['instrument'] for n in notes)
        if not notes and validation_error is None:validation_error='No valid recording-derived notes: '+kind
        for name in counts:programs[name]=0 if name=='drums' else model._program_for_instrument(name)
        if kind=='unconstrained':roster=[name for name,count in counts.most_common() if name!='voice' and count>=4][:10]
        write(kind+'-notes.json',dict(schema='reel.measured-note-pairs.v1',source=q['vocal_estimate'] if kind=='voice' else q['recording'],notes=notes,quarantined=outside,instruments=instruments,programs=programs,lyrics_used=False,source_modified=False,human_review=False,validation_passed=validation_error is None,validation_error=validation_error,raw_note_issues=diagnose_events(events)))
        passes.append(dict(kind=kind,instruments=instruments,note_count=len(notes),quarantined_count=len(outside),counts=dict(counts),elapsed_seconds=time.monotonic()-start,validation_passed=validation_error is None,validation_error=validation_error));print('PASS CAPTURED',kind,len(notes),'semantic validation',validation_error is None,flush=True)
    write('transcription-evidence.json',dict(schema='reel.full-recording-muscriptor-evidence.v1',golden_lock=q['golden_lock'],recording=q['recording'],vocal_estimate=q['vocal_estimate'],weights=q['weights'],model_config=q['model_config'],passes=passes,band_roster=roster,programs=programs,source_duration_seconds=q['duration_seconds'],all_measured_note_pairs_valid=all(p['validation_passed'] for p in passes),lyrics_used=False,private_only=True,external_upload=False,musical_approval=False,limitations=['Machine transcription and constrained reductions require musical review.','Detected source instruments only; unsupported layers must be omitted.','Events beyond the full recording are quarantined, never silently trimmed into measured notes.','A successful raw-capture job does not approve a failed note-span validation.']))
    print('REQUESTED RECORDING-DERIVED RAW PASSES CAPTURED',flush=True)


if __name__=='__main__':main()
