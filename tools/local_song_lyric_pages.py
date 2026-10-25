"""Bilingual reading pages anchored to retained performed-phrase clocks.

Canonical text stays separate from evidence; unmatched lines get no invented
clock. Repeated occurrences and long instrumental gaps remain explicit.
"""
import math
from bisect import bisect_right
import re
import unicodedata
from difflib import SequenceMatcher


def normalized(text):
    return ''.join(c for c in unicodedata.normalize('NFKD',text).casefold() if c.isalnum())


def event_occurrences(lines, events, allow_long_holds=False):
    chars='';owners=[]
    for i,e in enumerate(events):
        if not all(type(e[k]) in (float,int) and math.isfinite(e[k]) for k in ['start','end']) or e['end']<=e['start']:
            raise ValueError('Invalid lyric evidence clock')
        value=normalized(e['text']);chars+=value;owners.extend([i]*len(value))
    rows=[]
    for number,line in enumerate(lines,1):
        wanted=normalized(line['es']);found=[]
        for m in re.finditer(re.escape(wanted),chars):
            first,last=owners[m.start()],owners[m.end()-1]
            if not allow_long_holds and events[last]['end']-events[first]['start']>max(12,len(wanted)*.45):continue
            found.append(dict(line=number,start=events[first]['start'],end=events[last]['end'],
                              match='exact-normalized-event-text',score=1.,event_first=first,event_last=last))
        if not found:
            # Partial/altered performance text can anchor a canonical reading;
            # retain the literal evidence and score for producer inspection.
            candidates=[]
            for first in range(len(events)):
                value=''
                for last in range(first,min(len(events),first+len(wanted))):
                    if last>first and events[last]['start']-events[last-1]['end']>3:break
                    value+=normalized(events[last]['text'])
                    if len(value)>len(wanted)*1.35:break
                    if len(value)<max(10,len(wanted)*.65):continue
                    score=SequenceMatcher(None,wanted,value,autojunk=False).ratio()
                    if score>=.8 and events[last]['end']-events[first]['start']<=max(12,len(wanted)*.45):
                        candidates.append(dict(line=number,start=events[first]['start'],end=events[last]['end'],
                            match='partial-or-altered-event-text',score=score,event_first=first,event_last=last,evidence_text=value))
            for candidate in sorted(candidates,key=lambda x:-x['score']):
                if not any(min(candidate['end'],x['end'])>max(candidate['start'],x['start']) for x in found):found.append(candidate)
        rows.append(dict(line=number,pair=line,occurrences=sorted(found,key=lambda x:x['start'])))
    return rows


def timed_pages(occurrences, lines, track_start, track_end, sample_rate, wrap):
    ordered=sorted(occurrences,key=lambda x:(x['end'],x['start']))
    for row in ordered:
        if not 0<=row['start']<row['end']<=track_end-track_start+.1:raise ValueError('Phrase outside exact recording clock')
    ends=[x['end'] for x in ordered];scores=[0.];paths=[[]]
    for i,row in enumerate(ordered):
        previous=bisect_right(ends,row['start']+.2,0,i)
        weight=len(normalized(lines[row['line']-1]['es']))*row.get('score',1)**4
        candidate=scores[previous]+weight
        if candidate>scores[-1]:scores.append(candidate);paths.append(paths[previous]+[row])
        else:scores.append(scores[-1]);paths.append(paths[-1])
    accepted=paths[-1]
    chunks=[];chunk=[];rows=0
    for row in accepted:
        pair=lines[row['line']-1];needed=max(len(wrap(pair['es'])),len(wrap(pair['en'])))
        if needed>7:raise ValueError('Bilingual phrase exceeds phone page')
        if chunk and (rows+needed>7 or len(chunk)>=3 or row['start']-chunk[-1]['end']>3 or row['line']<=chunk[-1]['line'] or row['start']-chunk[0]['start']>14):
            chunks.append(chunk);chunk=[];rows=0
        chunk.append(row);rows+=needed
    if chunk:chunks.append(chunk)
    pages=[]
    for i,chunk in enumerate(chunks):
        start=max(track_start,track_start+chunk[0]['start']-.4,track_start+max(x['end'] for x in chunks[i-1])-.2 if i else track_start)
        stop=min(track_end,track_start+max(x['end'] for x in chunk)+1.0)
        if i+1<len(chunks):stop=min(stop,max(track_start+chunks[i+1][0]['start']-.4,track_start+max(x['end'] for x in chunk)-.2))
        if stop<=start:raise ValueError('Overlapping lyric page clocks')
        pages.append(dict(start_frame=round(start*sample_rate),end_frame=round(stop*sample_rate),
                          lines=[dict(lines[x['line']-1],canonical_line_number=x['line']) for x in chunk],
                          phrase_evidence=chunk,timing='performed-phrase-entrances',lead_seconds=.4))
    return pages,accepted
