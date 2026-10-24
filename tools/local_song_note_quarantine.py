"""Explicit bounded valid-note projections, preserving failed raw captures.

No start/end/pitch is repaired. Every rejected identity and its raw events are
retained in a ledger. A projected subset never upgrades its original capture.
"""
import collections
import json
import math
from pathlib import Path
from local_song_backend import checked, read, write, offline


def project(events,duration,programs,maximum_rejected=32,maximum_fraction=.005):
    if duration<=0 or not 0<=maximum_fraction<=.01 or not 0<=maximum_rejected<=32:
        raise ValueError('Unbounded or invalid quarantine policy')
    starts=collections.defaultdict(list);ends=collections.defaultdict(list)
    for event in events:
        if event['type']=='start':starts[event['index']].append(event)
        elif event['type']=='end':ends[event['start_event_index']].append(event)
        else:raise ValueError('Unknown raw note event type')
    notes=[];rejected=[]
    identities=sorted(starts.keys()|ends.keys())
    for identifier in identities:
        begin=starts[identifier];finish=ends[identifier];reason=None
        if len(begin)!=1 or len(finish)!=1:reason='ambiguous-or-missing-event-pair'
        else:
            start=begin[0]['start_time'];end=finish[0]['end_time'];pitch=begin[0]['pitch']
            if not all(type(v) in [int,float] and math.isfinite(v) for v in [start,end]) or not 0<=start<end<=duration:reason='invalid-or-outside-measured-span'
            elif type(pitch)!=int or not 0<=pitch<=127:reason='invalid-pitch'
            elif begin[0]['instrument'] not in programs:raise ValueError('Undeclared source instrument program')
        if reason:
            rejected.append(dict(id=identifier,reason=reason,start_events=begin,end_events=finish))
        else:notes.append(dict(id=identifier,start=start,end=end,pitch=pitch,instrument=begin[0]['instrument']))
    fraction=len(rejected)/max(1,len(identities))
    if not notes or len(rejected)>maximum_rejected or fraction>maximum_fraction:
        raise ValueError(f'Quarantine exceeds explicit bound: {len(rejected)}/{len(identities)} identities')
    return sorted(notes,key=lambda n:(n['start'],n['id'])),rejected,dict(raw_note_identities=len(identities),retained_notes=len(notes),rejected_notes=len(rejected),rejected_fraction=fraction,maximum_rejected=maximum_rejected,maximum_fraction=maximum_fraction,no_durations_or_pitches_manufactured=True,original_capture_status_unchanged=True)


def main():
    offline();q=read('quarantine-request.json')
    if q['schema']!='reel.measured-note-subset-request.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Explicit bounded private subset request required')
    checked('events.json',q['raw_events']);checked('programs.json',q['programs'])
    programs=read('programs.json');notes,rejected,proof=project(read('events.json'),q['duration_seconds'],programs,q['maximum_rejected'],q['maximum_fraction'])
    write('notes.json',dict(schema='reel.measured-note-pairs.v1',source=q['recording'],notes=notes,quarantined=rejected,instruments=sorted({n['instrument'] for n in notes}),programs=programs,lyrics_used=False,source_modified=False,human_review=False,validation_passed=True,validation_scope='Every retained pair is valid; original raw capture remains failed and unchanged',validation_error=None,raw_note_issues=[],subset_projection=proof))
    write('quarantine-ledger.json',dict(source_events=q['raw_events'],original_failed_capture=q['original_capture'],proof=proof,rejected=rejected,original_recording_unchanged=True,musical_approval=False))
    print('BOUNDED NOTE SUBSET RETAINED',proof['retained_notes'],'REJECTED',proof['rejected_notes'],flush=True)


if __name__=='__main__':main()
