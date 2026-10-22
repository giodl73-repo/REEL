"""Source-clock line-to-note candidates, never forced lyric underlay.

Each lexical candidate comes from one independent recognition pass. Accepted
performance omissions cannot be reassigned to similar phrases elsewhere.
"""
from local_song_backend import checked, offline, read, write
from local_song_occurrences import occurrences


def align(performed, canonical, analysis, model, omitted=(), confirmed_present=()):
    source=[s for s in performed.splitlines() if s.strip()]
    canon=[s for s in canonical.splitlines() if s.strip()]
    if len(source)!=len(canon):
        raise ValueError('Canonical/performed line cardinality differs; explicit correspondence required')
    if set(omitted)&set(confirmed_present) or any(type(n) is not int or not 1<=n<=len(source) for n in [*omitted,*confirmed_present]):
        raise ValueError('Invalid or conflicting owner line decisions')
    tempo=model['tempo_map']
    if len(tempo)!=1 or tempo[0]['tick']!=0:
        raise ValueError('Variable tempo needs explicit source-clock conversion')
    scale=tempo[0]['microseconds_per_quarter']/1e6/model['musical_timebase']['pulses_per_quarter']
    duration=model['duration_ticks']*scale
    notes=[dict(id=n['id'],start=n['start_tick']*scale,end=(n['start_tick']+n['duration_ticks'])*scale)
           for part in model['parts'] for n in part['notes']]
    rows=[]
    for row,text in zip(occurrences(performed,analysis['passes']),canon):
        number=row['line'];candidates=[]
        if number not in omitted:
            for found in row['candidates']:
                start,end=found['start'],found['end']
                if start<0 or end<=start or end>duration+.02:
                    continue
                if found['missing_words'] and len(source[number-1].split())<=2:
                    continue
                if end-start>max(4,len(source[number-1].split())*3):
                    continue
                ids=[n['id'] for n in notes if min(end,n['end'])-max(start,n['start'])>=.04]
                candidate=dict(found,note_run_ids=ids,lexical_status='exact' if found['missing_words']==0 else 'one-word-gap')
                # Keep the best single-pass representative of duplicate clocks;
                # no words are combined to manufacture a complete phrase.
                duplicate=next((x for x in candidates if abs(x['start']-start)<.75 and abs(x['end']-end)<.75),None)
                if duplicate:
                    if (candidate['missing_words'],candidate['end']-candidate['start'])<(duplicate['missing_words'],duplicate['end']-duplicate['start']):
                        candidates.remove(duplicate);candidates.append(candidate)
                else:candidates.append(candidate)
        candidates.sort(key=lambda x:x['start'])
        if number in omitted:status='owner-accepted-performance-omission; no underlay'
        elif candidates:status='source-clock candidates; lyric underlay unassigned'
        elif number in confirmed_present:status='owner-confirmed present; exact clock unresolved'
        else:status='recognition clock unresolved'
        rows.append(dict(line=number,canonical=text,performed_source_line=source[number-1],
                         canonical_wording_differs=text.strip()!=source[number-1].strip(),
                         status=status,candidates=candidates))
    return dict(schema='reel.local-song-line-alignment.v1',lines=rows,
                lines_with_clock_and_notes=sum(any(c['note_run_ids'] for c in r['candidates']) for r in rows),
                unresolved_lines=[r['line'] for r in rows if not r['candidates'] and r['line'] not in omitted],
                owner_accepted_omissions=list(omitted),underlay_assigned=False,
                source_audio_modified=False,pitches_modified=False,
                limitations=['Recognition clocks and stable F0 runs are hypotheses, not syllable boundaries.',
                             'Repeated note articulation and melisma require separate review.',
                             'Complete canonical text remains separate from actual singing.'])


def main():
    offline();request=read('alignment.json')
    if request['schema']!='reel.local-song-line-alignment-request.v1':
        raise ValueError('Unsupported alignment request')
    for name,key in [('performed.txt','performed'),('canonical.txt','canonical'),('analysis.json','analysis'),('model.json','model'),('transcription.json','transcription_request')]:
        checked(name,request[key])
    analysis,model=read('analysis.json'),read('model.json')
    transcription=read('transcription.json')
    if analysis['recording']['sha256']!=request['recording']['sha256'] or transcription['recording']['sha256']!=request['recording']['sha256'] or transcription['candidate_id']!=model['model_id']:
        raise ValueError('Recognition is bound to a different recording')
    from pathlib import Path
    result=align(Path('performed.txt').read_text(encoding='utf-8'),Path('canonical.txt').read_text(encoding='utf-8'),
                 analysis,model,request.get('owner_omitted_lines',[]),request.get('owner_confirmed_present_lines',[]))
    result['request']=request
    write('line-alignment.json',result)
    print('SOURCE-CLOCK LINE CANDIDATES',result['lines_with_clock_and_notes'],'with notes; no underlay or pitch edits',flush=True)


if __name__=='__main__':main()
