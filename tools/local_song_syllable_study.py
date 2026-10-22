"""Offline character-timed syllable studies, separate from accepted melodies.

Expected text constrains CTC alignment: this estimates timing, NEVER presence.
Only independently exact, nonoverlapping phrase candidates enter a study.
The native renderer owns notation exports. Uncovered pitch hypotheses remain
in the unchanged parent model and the explicit exclusion ledger.
"""
import copy
import math
import os
from pathlib import Path
import re
import subprocess
import unicodedata

from local_song_backend import checked, descriptor, offline, read, write
from local_song_transcribe import native


def syllables(word):
    """Orthographic proposal, not a claim about the singer's vowel grouping.

    Original characters are preserved. Intervocalic h, prefix boundaries,
    variable tl division and musical elision require review.
    """
    text = word.lower()
    vowels = set('aeiouáéíóúü')
    positions = [i for i,c in enumerate(text) if c in vowels or c=='y' and i==len(text)-1]
    # u is silent after q/g before e/i, except ü.
    positions = [i for i in positions if not (text[i]=='u' and i>0 and i+1<len(text)
                  and text[i-1] in 'qg' and text[i+1] in 'eiéí')]
    if not positions:
        return [(0,len(word))]
    nuclei = [[positions[0]]]
    plain = lambda c: ''.join(x for x in unicodedata.normalize('NFD', c) if not unicodedata.combining(x))
    for pos in positions[1:]:
        prior = nuclei[-1][-1]
        a,b = text[prior],text[pos]
        adjacent = pos==prior+1 or text[prior+1:pos]=='h'
        hiatus = (plain(a)==plain(b) or a in 'íú' and plain(b) in 'aeo' or b in 'íú' and plain(a) in 'aeo'
                  or plain(a) in 'aeo' and plain(b) in 'aeo')
        if adjacent and not hiatus and len(nuclei[-1])<3:
            nuclei[-1].append(pos)
        else:
            nuclei.append([pos])
    cuts = [0]
    inseparable = {'bl','br','cl','cr','dr','fl','fr','gl','gr','pl','pr','tr','ch','ll','rr','qu','gu'}
    for left,right in zip(nuclei,nuclei[1:]):
        a,b = left[-1],right[0]
        between = text[a+1:b]
        if not between:
            cut=b
        elif len(between)==1:
            cut=a+1
        elif between[-2:] in inseparable:
            cut=b-2
        else:
            cut=b-1
        cuts.append(cut)
    cuts.append(len(word))
    return list(zip(cuts,cuts[1:]))


def choose_phrases(report):
    """Weighted interval schedule; retains repeats but not alternative clocks.

    Producer proposal only. No source-line ordering is imposed on the singing.
    """
    from bisect import bisect_right
    eligible=[]
    for phrase in report['phrases']:
        if phrase['line'] in report['owner_accepted_omissions']:continue
        items=phrase['words']
        if not items or any(w['token_index'] is None or 'source_span_seconds' not in w
                            or not 0<w['source_span_seconds'][1]-w['source_span_seconds'][0]<=3
                            or 'token-reused' in w['status'] for w in items):
            continue
        start,end=phrase['span_seconds']
        score=len(items)*100 + min(30,sum(len(w['note_ids']) for w in items)) - (end-start)
        eligible.append((end,start,phrase['id'],score,phrase))
    eligible.sort(key=lambda x:x[:3]);ends=[x[0] for x in eligible]
    best=[(0,[])]
    for i,(end,start,_,score,phrase) in enumerate(eligible):
        prior=bisect_right(ends,start,0,i)
        take=(best[prior][0]+score,best[prior][1]+[phrase])
        best.append(take if take[0]>best[-1][0] else best[-1])
    return best[-1][1]


def char_spans(path, target, labels, step, offset):
    """Recover every constrained character, preserving repeat/blank semantics."""
    result=[];prior=0
    for frame,label in enumerate(path):
        if label and label!=prior:
            index=len(result)
            if index>=len(target) or labels[label]!=target[index]:
                raise ValueError('Forced CTC path does not reproduce its expected characters')
            result.append([offset+frame*step,offset+(frame+1)*step])
        elif label:
            result[-1][1]=offset+(frame+1)*step
        prior=label
    if len(result)!=len(target):
        raise ValueError('Incomplete constrained character path')
    return result


def derived_model(parent, syllable_rows, expected_text, title):
    """Exact interval intersections, never nearest-note or invented pitch fill."""
    model=copy.deepcopy(parent)
    scale=parent['tempo_map'][0]['microseconds_per_quarter']/1e6/parent['musical_timebase']['pulses_per_quarter']
    originals=[n for p in parent['parts'] for n in p['notes']]
    notes=[];underlay=[];ledger=[]
    for row in syllable_rows:
        ids=[]
        if row['mean_log_probability'] < -4:
            row['status']='low-acoustic-support; unmapped'
            continue
        a,b=row['span_seconds']
        for original in originals:
            start=max(original['start_tick'],math.ceil(a/scale))
            end=min(original['start_tick']+original['duration_ticks'],math.floor(b/scale))
            if (end-start)*scale<.04:
                continue
            note=copy.deepcopy(original);note['id']=f'study-note-{len(notes):04}'
            note['start_tick']=start;note['duration_ticks']=end-start
            note['provenance']['rationale']='Partial-study intersection of parent pitch run with expected-text CTC timing; articulation unreviewed'
            notes.append(note);ids.append(note['id'])
            ledger.append(dict(note_id=note['id'],parent_note_id=original['id'],
                               parent_start_tick=original['start_tick'],parent_duration_ticks=original['duration_ticks'],
                               proposed_start_tick=start,proposed_duration_ticks=end-start))
        row['note_ids']=ids
        row['status']='syllable-to-note timing proposal' if ids else 'no-recovered-pitch; unmapped'
        if ids:
            underlay.append(dict(id=f'syllable-{len(underlay):04}',note_ids=ids,
                                 text_start_byte=row['text_start_byte'],text_end_byte=row['text_end_byte'],
                                 syllabic=row['syllabic']))
    if not notes:
        raise ValueError('No supported syllable/pitch intersections; no empty study score')
    if any(notes[i]['start_tick']+notes[i]['duration_ticks']>notes[i+1]['start_tick'] for i in range(len(notes)-1)):
        raise ValueError('Study articulation overlaps; review phrase/character boundaries')
    model['model_id']+='-partial-syllable-study-v1'
    authority=copy.deepcopy(model['authority']);authority['status']='draft-not-reviewed';authority['decision_refs']=[]
    model['authority']=authority;model['review']['status']='not-reviewed';model['review']['decision_refs']=[]
    model['parts']=[dict(id='partial-vocal-study',role='melody',name='Partial syllable study — gaps unresolved',notes=notes)]
    model['lyric_layers'].append(dict(id='expected-singing-study',kind='as-sung',language='es',path='expected-singing.txt',
        sha256=__import__('hashlib').sha256(expected_text.encode('utf-8')).hexdigest(),authority=authority))
    model['lead_sheet']=dict(title=title+' — PARTIAL SYLLABLE STUDY; NOT COMPLETE SCORE',
                            melody_part_id='partial-vocal-study',lyric_layer_id='expected-singing-study',underlay=underlay)
    model['expressive_timing']=[]
    model['unknowns']+=['Partial study omits uncovered parent hypotheses; parent model remains unchanged.',
                        'Expected-text forced alignment estimates timing, never independent lyric presence.',
                        'Orthographic syllable groups, consonant timing and proposed note articulation require musical review.',
                        'Unmapped syllables are gaps, not evidence that the singer omitted them.']
    return model,ledger


def main():
    offline();request=read('syllable.json');runtime=read('runtime.json');component=read('component.json')
    if request['schema']!='reel.local-song-syllable-study-request.v1':raise ValueError('Unsupported request')
    if request['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Private offline study only')
    if component['schema']!='reel.installed-spanish-ctc.v1':raise ValueError('Unsupported installed CTC component')
    for path,key in [('word-underlay.json','word_underlay'),('parent-model.json','model'),('vocals.wav','vocal_estimate'),
                     ('transcription.json','transcription_request'),('reel.exe','reel_runtime')]:checked(path,request[key])
    report=read('word-underlay.json');parent=read('parent-model.json');transcription=read('transcription.json')
    if report['request']['model']!=request['model'] or report['request']['recording']!=request['recording'] or transcription['recording']!=request['recording'] or transcription['vocal_estimate']!=request['vocal_estimate']:
        raise ValueError('Full source/stem/model binding differs')
    if len(parent['tempo_map'])!=1 or parent['tempo_map'][0]['tick']!=0:raise ValueError('Explicit constant source-clock map required')
    checked(runtime['python']['path'],runtime['python']);checked(runtime['ffmpeg']['path'],runtime['ffmpeg']);model_path=checked(component['model']['path'],component['model'])
    import numpy as np
    import torch
    import torchaudio
    if torchaudio.__version__!=component['torchaudio_version']:raise ValueError('CTC runtime version differs')
    torch.set_num_threads(2);torch.manual_seed(0)
    def no_download(*a,**k):raise RuntimeError('Offline: download forbidden')
    torch.hub.download_url_to_file=no_download
    bundle=torchaudio.pipelines.VOXPOPULI_ASR_BASE_10K_ES
    if Path(bundle._path).name!=model_path.name:raise ValueError('Frozen CTC model differs')
    acoustic=bundle.get_model(dl_kwargs=dict(model_dir=str(model_path.parent),file_name=model_path.name,map_location='cpu',weights_only=True)).eval()
    labels=bundle.get_labels();lookup={c:i for i,c in enumerate(labels)}
    selected=choose_phrases(report);rows=[];phrases=[];expected_text=''
    duration=parent['duration_ticks']*parent['tempo_map'][0]['microseconds_per_quarter']/1e6/parent['musical_timebase']['pulses_per_quarter']
    for phrase in selected:
        start,end=phrase['span_seconds'];start=max(0,start-.5);end=min(duration,end+.5)
        text=phrase['performed_source_line'].strip()
        matches=list(re.finditer(r'[^\W\d_]+',text,re.UNICODE))
        target='|'.join(''.join(c for c in unicodedata.normalize('NFKD',m.group().lower()) if not unicodedata.combining(c)) for m in matches)
        if any(c not in lookup for c in target):raise ValueError('Unsupported expected-text character; no silent substitution')
        print('EXPECTED-TEXT TIMING',phrase['id'],round(start,2),round(end,2),flush=True)
        data=subprocess.check_output([runtime['ffmpeg']['path'],'-v','error','-ss',str(start),'-t',str(end-start),'-i','vocals.wav','-ac','1','-ar','16000','-f','f32le','-'])
        waveform=np.frombuffer(data,dtype=np.float32).copy()
        if not np.isfinite(waveform).all() or abs(len(waveform)/16000-(end-start))>.05:raise ValueError('Invalid source window')
        with torch.inference_mode():
            emission,_=acoustic(torch.from_numpy(waveform)[None]);probs=emission.log_softmax(-1)
            path,scores=torchaudio.functional.forced_align(probs,torch.tensor([[lookup[c] for c in target]],dtype=torch.int64),blank=0)
        ids=path[0].tolist();step=len(waveform)/16000/len(ids)
        characters=char_spans(ids,target,labels,step,start)
        prefix=len(expected_text.encode('utf-8'));expected_text+=text+'\n'
        cursor=0;wordrows=[]
        for match in matches:
            word=match.group();cuts=syllables(word);charclock=characters[cursor:cursor+len(word)]
            if len(charclock)!=len(word):raise ValueError('Explicit Unicode character correspondence required')
            boundaries=[max(phrase['span_seconds'][0],charclock[0][0])]
            boundaries += [(charclock[b-1][1]+charclock[b][0])/2 for _,b in cuts[:-1]]
            boundaries += [min(phrase['span_seconds'][1],charclock[-1][1])]
            for i,(a,b) in enumerate(cuts):
                low,high=boundaries[i:i+2]
                mask=[j for j,lab in enumerate(ids) if lab and characters[cursor+a][0]<=start+j*step<characters[cursor+b-1][1]]
                score=float(scores[0,mask].mean()) if mask else -100.0
                row=dict(phrase_id=phrase['id'],line=phrase['line'],word=word,text=word[a:b],
                         text_start_byte=prefix+len(text[:match.start()+a].encode('utf-8')),
                         text_end_byte=prefix+len(text[:match.start()+b].encode('utf-8')),
                         syllabic='single' if len(cuts)==1 else 'begin' if i==0 else 'end' if i==len(cuts)-1 else 'middle',
                         span_seconds=[low,high],mean_log_probability=score,note_ids=[])
                if high>low:rows.append(row);wordrows.append(row)
            cursor+=len(word)+1
        phrases.append(dict(id=phrase['id'],line=phrase['line'],source_span_seconds=phrase['span_seconds'],
            performed_source_line=text,expected_characters=target,character_spans=characters,
            frame_seconds=step,forced_path=ids,selection='producer-proposed nonoverlapping exact-recognizer phrase; musical review pending'))
    Path('expected-singing.txt').write_text(expected_text,encoding='utf-8',newline='\n')
    model,ledger=derived_model(parent,rows,expected_text,request['title'])
    write('model.json',model)
    used={x['parent_note_id'] for x in ledger}
    evidence=dict(schema='reel.local-song-syllable-study.v1',request=request,phrases=phrases,syllables=rows,
        articulation_ledger=ledger,parent_notes_total=sum(len(p['notes']) for p in parent['parts']),
        parent_note_ids_with_partial_support=sorted(used),uncovered_parent_note_ids=[n['id'] for p in parent['parts'] for n in p['notes'] if n['id'] not in used],
        owner_accepted_omissions=report['owner_accepted_omissions'],unresolved_line_clocks=report['unresolved_line_clocks'],
        original_audio_modified=False,parent_model_modified=False,independent_presence_test=False,finished_score=False,
        limitations=model['unknowns'])
    write('syllable-study.json',evidence)
    checks=dict(model=native('music-model-validate','model.json'))
    native('music-score-export-plan','model.json','--output-path','score-plan.json')
    native('music-score-export-render','score-plan.json','model.json','--output-dir','rendered')
    checks['score_export']=native('music-score-export-check','rendered/receipt.json','score-plan.json','model.json','rendered')
    write('native-checks.json',checks)
    for filename in ['score.mid','score.musicxml','lead-sheet.svg','rehearsal-guide.wav','receipt.json']:os.link(Path('rendered')/filename,filename)
    print('NATIVE PARTIAL SYLLABLE STUDY VERIFIED',len(model['lead_sheet']['underlay']),'mapped syllables;',len(used),'parent runs with partial support',flush=True)


if __name__=='__main__':main()
