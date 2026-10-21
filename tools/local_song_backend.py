"""Explicit offline ACE song generation and unprompted lyric diagnostics.

Run through reel-assets run-job. Private requests and evidence are project data.
No cache implementation, download, service, selection or release lives here.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import unicodedata
import zipfile

ASR_BATCH_SIZE = 4


def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def read(path):
    return json.loads(Path(path).read_text(encoding='utf-8-sig'))


def write(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(data, stream, ensure_ascii=False, indent=2)
        stream.write('\n')


def descriptor(path):
    return dict(sha256=digest(path), bytes=Path(path).stat().st_size)


def checked(path, desc):
    path = Path(path)
    if not path.is_file() or path.stat().st_size != desc['bytes'] or digest(path) != desc['sha256']:
        raise ValueError(f'Pinned file missing or changed: {path}')
    return path


def offline():
    for key in ['HF_HUB_OFFLINE', 'TRANSFORMERS_OFFLINE', 'HF_DATASETS_OFFLINE', 'HF_HUB_DISABLE_TELEMETRY']:
        os.environ[key] = '1'
    os.environ['DO_NOT_TRACK'] = '1'
    os.environ['TOKENIZERS_PARALLELISM'] = 'false'


def validate_request(request, source):
    if request['schema'] != 'reel.local-song-request.v1':
        raise ValueError('Unsupported song request')
    if request['task'] not in ('text2music','cover') or request['language'] != 'es':
        raise ValueError('Unsupported Spanish generation task')
    if request['task']=='cover':
        if not all(isinstance(request.get(key),dict) for key in ['guide_audio','guide_request','guide_generation','guide_version_lock']):
            raise ValueError('Cover requires declared generated-guide proof')
        strength=request.get('audio_cover_strength')
        if type(strength) not in (int,float) or not .2<=strength<=1 or request.get('semantic_planner'):
            raise ValueError('Unsupported generated-guide conditioning')
    if not 30 <= request['duration_seconds'] <= 600 or not isinstance(request['seed'], int):
        raise ValueError('Invalid bounded duration or seed')
    if source.read_bytes().decode('utf-8') != request['canonical_lyrics']:
        raise ValueError('Canonical lyrics differ from frozen source bytes')
    # Engine stanza tags may change layout, never authored words.
    rendered = re.sub(r'^\s*\[[^\]\n]+\]\s*$', '', request['render_lyrics'], flags=re.M)
    repetitions=request.get('performance_repetitions',1)
    if type(repetitions) is not int or repetitions not in (1,2):
        raise ValueError('Unsupported bounded lyric repetition count')
    section_lines=request.get('repeat_section_lines')
    if section_lines is not None:
        if type(section_lines) is not int or not 2<=section_lines<=8 or repetitions!=2:
            raise ValueError('Unsupported repeated section layout')
        expected=[w for section in lyric_sections(request['canonical_lyrics'],section_lines) for _ in range(2) for w in words(section)]
    else:expected=words(request['canonical_lyrics'])*repetitions
    if words(rendered) != expected:
        raise ValueError('Engine formatting changes lyric words')
    if request['permissions'] != dict(private_only=True, external_upload=False, identifiable_imitation=False, public_release=False):
        raise ValueError('Unsupported permission scope')
    if request.get('recognition_dtype','float32') not in ('float32','float16'):
        raise ValueError('Unsupported explicit recognition dtype')


def runtime_files(runtime, group):
    for item in runtime[group]:
        checked(item['path'], item)


def validate_generated_guide(request,source):
    checked('guide.wav',request['guide_audio'])
    checked('guide-request.json',request['guide_request'])
    checked('guide-generation.json',request['guide_generation'])
    original=read('guide-request.json');receipt=read('guide-generation.json')
    validate_request(original,source)
    if original['task']!='text2music' or original['source_lock']!=request['source_lock']:
        raise ValueError('Guide must be a generated text2music take of the same source version')
    if original['canonical_lyrics']!=request['canonical_lyrics'] or original['permissions']!=request['permissions']:
        raise ValueError('Guide source or private permission scope differs')
    if original['duration_seconds']!=request['duration_seconds']:
        raise ValueError('Cover duration must match its generated guide')
    if receipt['request']!=descriptor('guide-request.json') or receipt['recording']!=descriptor('guide.wav'):
        raise ValueError('Generated guide receipt binding differs')
    return dict(recording=descriptor('guide.wav'),request=descriptor('guide-request.json'),generation=descriptor('guide-generation.json'))


def plan(request, runtime, source):
    """Optional frozen local semantic codes; a separate process releases VRAM."""
    validate_request(request, source)
    component=read('planner.json')
    if component['schema']!='reel.local-song-planner.v1':
        raise ValueError('Unsupported planner component')
    if descriptor('planner.json')!={k:request['semantic_planner'][k] for k in ['sha256','bytes']}:
        raise ValueError('Planner differs from requested component')
    runtime_files(component,'files')
    checked(runtime['python']['path'],runtime['python'])
    actual=subprocess.check_output(['git','-C',runtime['ace_root'],'rev-parse','HEAD'],text=True).strip()
    if actual!=runtime['ace_commit']:raise ValueError('ACE source revision changed')
    sys.path.insert(0,runtime['ace_root'])
    import torch
    from acestep.llm_inference import LLMHandler
    torch.set_num_threads(4)
    torch.manual_seed(request['seed'])
    handler=LLMHandler();start=time.time()
    status,ok=handler.initialize(checkpoint_dir=component['checkpoint_dir'],lm_model_path=component['model_name'],
        backend='pt',device='cuda',offload_to_cpu=False,dtype=torch.float32)
    if not ok:raise RuntimeError(str(status))
    result=handler.generate_with_stop_condition(caption=request['caption'],lyrics=request['render_lyrics'],
        infer_type='llm_dit',temperature=.85,cfg_scale=1.0,target_duration=request['duration_seconds'],
        user_metadata=dict(duration=request['duration_seconds'],language='es',caption=request['caption']),
        use_cot_caption=False,use_cot_language=False,use_cot_metas=False,
        use_constrained_decoding=True,batch_size=1,seeds=[request['seed']])
    if not result.get('success'):raise RuntimeError(str(result.get('error')))
    codes=result.get('audio_codes','')
    count=len(re.findall(r'<\|audio_code_\d+\|>',codes)) if isinstance(codes,str) else 0
    if abs(count-5*request['duration_seconds'])>10:raise ValueError('Unexpected semantic code duration')
    for key in ['lyrics','canonical_lyrics']:
        if result.get('metadata',{}).get(key) and result['metadata'][key] not in [request['render_lyrics'],request['canonical_lyrics']]:
            raise ValueError('Planner lyric rewriting is prohibited')
    write('plan.json',dict(schema='reel.local-song-plan.v1',request=descriptor('request.json'),
        component=descriptor('planner.json'),audio_codes=codes,code_count=count,metadata=result.get('metadata',{}),
        dtype='float32',backend='pt',elapsed_seconds=round(time.time()-start,3),external_upload=False))


def generate(request, runtime, source):
    validate_request(request, source)
    guide=validate_generated_guide(request,source) if request['task']=='cover' else None
    runtime_files(runtime, 'generation_files')
    checked(runtime['python']['path'], runtime['python'])
    actual = subprocess.check_output(['git', '-C', runtime['ace_root'], 'rev-parse', 'HEAD'], text=True).strip()
    if actual != runtime['ace_commit']:
        raise ValueError('ACE source revision changed')
    os.environ['ACESTEP_ROCM_DTYPE'] = 'float32'
    sys.path.insert(0, runtime['ace_root'])
    import torch
    import acestep.gpu_config as gpu_config
    gpu_config.is_rocm_available = lambda: True
    from acestep.handler import AceStepHandler
    from acestep.inference import GenerationConfig, GenerationParams, generate_music
    torch.set_num_threads(4)
    handler = AceStepHandler()
    start = time.time()
    print('Initialize explicit offline ACE float32/eager runtime', flush=True)
    status, ok = handler.initialize_service(
        project_root=runtime['ace_root'], config_path='acestep-v15-turbo',
        device='cuda', use_flash_attention=False, compile_model=False,
        offload_to_cpu=True, offload_dit_to_cpu=False,
        quantization='w8a8_dynamic', use_mlx_dit=False,
    )
    if not ok or handler.dtype is not torch.float32:
        raise RuntimeError(f'ACE initialization failed: {status}')
    params = GenerationParams(
        task_type=request['task'], caption=request['caption'], lyrics=request['render_lyrics'],
        instrumental=False, vocal_language='es', bpm=None, keyscale='', timesignature='',
        duration=request['duration_seconds'], inference_steps=8, guidance_scale=7.5,
        seed=request['seed'], thinking=False, use_cot_metas=False,
        use_cot_caption=False, use_cot_language=False, use_cot_lyrics=False,
    )
    if guide:
        params.src_audio='guide.wav'
        params.audio_cover_strength=request['audio_cover_strength']
    if request.get('semantic_planner'):
        planned=read('plan.json')
        if planned['request']!=descriptor('request.json'):raise ValueError('Plan request binding changed')
        params.audio_codes=planned['audio_codes']
    print('Generate', request['candidate_id'], 'seed', params.seed, flush=True)
    result = generate_music(handler, None, params,
        GenerationConfig(batch_size=1, use_random_seed=False, seeds=[params.seed], audio_format='wav'),
        save_dir='generated')
    if not result.success or len(result.audios) != 1:
        raise RuntimeError(str(result.error or 'Expected exactly one generated recording'))
    os.link(result.audios[0]['path'], 'recording.wav')
    write('generation.json', dict(
        schema='reel.local-song-generation.v1', request=descriptor('request.json'),
        source=descriptor(source), runtime=descriptor('runtime.json'),
        candidate_id=request['candidate_id'], seed=params.seed,
        duration_seconds=request['duration_seconds'], recording=descriptor('recording.wav'),
        elapsed_seconds=round(time.time()-start, 3), ace_commit=actual,
        initialization=str(status), dtype=str(handler.dtype),
        quantization='w8a8_dynamic', compile_model=False, semantic_planner=bool(request.get('semantic_planner')),
        plan=descriptor('plan.json') if request.get('semantic_planner') else None,
        task_type=request['task'],audio_conditioning=guide,
        audio_cover_strength=params.audio_cover_strength if guide else None,
        cover_noise_strength=params.cover_noise_strength if guide else None,
        duration_locked_to_guide=bool(guide),
        external_upload=False, human_listening='pending', selected=False,
    ))
    print('Recording retained for technical and lyric analysis', flush=True)


def words(text):
    normalized = ''.join(c for c in unicodedata.normalize('NFKD', text.lower()) if not unicodedata.combining(c))
    return re.findall(r'[a-z]+', normalized)


def lyric_sections(lyrics,section_lines):
    sections=[]
    for stanza in re.split(r'\r?\n[ \t]*\r?\n',lyrics.strip()):
        lines=[line.strip() for line in stanza.splitlines() if line.strip()]
        sections.extend('\n'.join(lines[i:i+section_lines]) for i in range(0,len(lines),section_lines))
    return sections


def format_repeated_sections(lyrics,section_lines,max_phrase_words=None):
    parts=[]
    for section in lyric_sections(lyrics,section_lines):
        rendered=format_lyrics(section,True,max_phrase_words).removesuffix('\n[Outro - instrumental]')
        parts.extend([rendered,rendered])
    return '\n'.join(parts)+'\n[Outro - instrumental]'


def format_lyrics(lyrics,join_single_word_continuations=False,max_phrase_words=None):
    """Musical line wrapping only; canonical bytes and word order stay separate."""
    stanzas=[]
    for stanza in re.split(r'\r?\n[ \t]*\r?\n',lyrics.strip()):
        if not stanza.strip():continue
        lines=[]
        for line in stanza.splitlines():
            text=line.strip()
            join_short=max_phrase_words is not None and lines and len(words(lines[-1]))+len(words(text))<=max_phrase_words
            if lines and not re.search(r'[.!?;:]$',lines[-1]) and (join_short or (join_single_word_continuations and len(words(text))==1)):
                lines[-1]+=' '+text
            else:lines.append(text)
        stanzas.append('[Verse]\n'+'\n'.join(lines))
    return '\n'.join(stanzas)+'\n[Outro - instrumental]'


def ordered_matches(expected, tokens):
    """LCS preserves canonical order, skipping allowed performed repetitions.

    Every observed token is used at most once. Accents/case/punctuation are
    normalized; spelling substitutions are not silently counted as matches.
    """
    n, m = len(expected), len(tokens)
    if n > 4000 or m > 10000:
        raise ValueError('Lyric alignment exceeds bounded token budget')
    from array import array
    rows = [array('H', [0]) * (m+1) for _ in range(n+1)]
    for i in range(n-1, -1, -1):
        row, below = rows[i], rows[i+1]
        for j in range(m-1, -1, -1):
            row[j] = 1 + below[j+1] if expected[i] == tokens[j] else max(below[j], row[j+1])
    result = {}
    i = j = 0
    while i < n and j < m:
        if expected[i] == tokens[j]:
            result[i] = j
            i += 1
            j += 1
        elif rows[i+1][j] >= rows[i][j+1]:
            i += 1
        else:
            j += 1
    return result


def ordered_line_report(lyrics, passes):
    expected = words(lyrics)
    mappings = [ordered_matches(expected, [t['word'] for t in p['tokens']]) for p in passes]
    lines = []
    offset = 0
    for number, text in enumerate([s for s in lyrics.splitlines() if s.strip()], 1):
        line = words(text)
        counts = [sum(i in mapping for i in range(offset, offset+len(line))) for mapping in mappings]
        best = max(range(len(counts)), key=lambda k: counts[k])
        mapping = mappings[best]
        missing = [w for i,w in enumerate(line, offset) if i not in mapping]
        spans = [passes[best]['tokens'][mapping[i]]['span'] for i in range(offset, offset+len(line)) if i in mapping]
        starts = [s[0] for s in spans if s[0] is not None]
        ends = [s[1] for s in spans if s[1] is not None]
        # In particular, an entirely absent one-word line must not qualify.
        qualifies = bool(line) and counts[best] > 0 and len(missing) <= 1
        lines.append(dict(line=number, canonical=text, total_words=len(line),
            matched_words=counts, best_pass=passes[best]['name'], missing_words=missing,
            recognized_span=[min(starts), max(ends)] if starts and ends else None,
            passes_gate=qualifies))
        offset += len(line)
    return dict(lines=lines, passed=bool(lines) and all(row['passes_gate'] for row in lines),
        total_lines=len(lines), qualifying_lines=sum(row['passes_gate'] for row in lines),
        exact_lines=sum(not row['missing_words'] for row in lines),
        one_word_gap_lines=sum(len(row['missing_words']) == 1 and row['passes_gate'] for row in lines),
        repeated_lines_allowed=True, max_unrecognized_words_per_line=1,
        evidence='Best complete pass per source line; unprompted recognition, not proof of exact singing',
        human_listening='pending', selected=False)


def phrase_candidates(expected,tokens):
    """Near-contiguous source phrases, not words collected across a song.

    At most one source word may be absent and at most two extra recognized words
    may intervene. Observed words cannot be reused for two source occurrences.
    """
    from bisect import bisect_left
    positions={}
    for i,t in enumerate(tokens):positions.setdefault(t['word'],[]).append(i)
    result={}
    omissions=[None]+list(range(len(expected))) if len(expected)>1 else [None]
    for omitted in omissions:
        target=[(i,w) for i,w in enumerate(expected) if i!=omitted]
        if not target:continue
        for start in positions.get(target[0][1],[]):
            found={};cursor=start
            for i,w in target:
                points=positions.get(w,[]);p=bisect_left(points,cursor)
                if p==len(points) or points[p]-start+1>len(target)+2:break
                found[i]=points[p];cursor=points[p]+1
            if len(found)!=len(target):continue
            first,last=min(found.values()),max(found.values())
            t0,t1=tokens[first]['span'][0],tokens[last]['span'][1]
            if t0 is not None and t1 is not None and t1-t0>max(8,3*len(expected)):continue
            key=tuple(sorted(found.items()))
            result[key]=found
    return sorted(result.values(),key=lambda m:(-len(m),max(m.values())-min(m.values())+1-len(m),min(m.values())))


def line_report(lyrics,passes):
    """Unique-line presence across repeated verses; order is a separate screen."""
    source=[s for s in lyrics.splitlines() if s.strip()]
    ww=[words(s) for s in source]
    mappings=[]
    for p in passes:
        choices=[phrase_candidates(line,p['tokens']) for line in ww]
        order=sorted(range(len(source)),key=lambda i:(len(ww[i])==1,len(choices[i]) or 100000,-len(ww[i])))
        used=set();assigned=[{} for _ in source]
        for i in order:
            for candidate in choices[i]:
                if not used.intersection(candidate.values()):
                    assigned[i]=candidate;used.update(candidate.values());break
        mappings.append(assigned)
    lines=[]
    for i,text in enumerate(source):
        counts=[len(mapping[i]) for mapping in mappings]
        best=max(range(len(counts)),key=lambda k:counts[k]);mapping=mappings[best][i]
        missing=[w for j,w in enumerate(ww[i]) if j not in mapping]
        spans=[passes[best]['tokens'][j]['span'] for j in mapping.values()]
        starts=[s[0] for s in spans if s[0] is not None];ends=[s[1] for s in spans if s[1] is not None]
        lines.append(dict(line=i+1,canonical=text,total_words=len(ww[i]),matched_words=counts,
            best_pass=passes[best]['name'],missing_words=missing,
            recognized_span=[min(starts),max(ends)] if starts and ends else None,
            passes_gate=bool(ww[i]) and counts[best]>0 and len(missing)<=1))
    ordered=ordered_line_report(lyrics,passes)
    return dict(lines=lines,passed=bool(lines) and all(r['passes_gate'] for r in lines),
        total_lines=len(lines),qualifying_lines=sum(r['passes_gate'] for r in lines),
        exact_lines=sum(not r['missing_words'] for r in lines),
        one_word_gap_lines=sum(len(r['missing_words'])==1 and r['passes_gate'] for r in lines),
        repeated_lines_allowed=True,max_unrecognized_words_per_line=1,
        coverage_recipe='repeat-aware-near-contiguous-phrases-v2',
        continuous_order_passed=ordered['passed'],continuous_order_qualifying_lines=ordered['qualifying_lines'],
        source_order_diagnostic=ordered,
        evidence='Best complete recognition pass per near-contiguous source phrase; distinct observed tokens; different repeated occurrences may supply different lines. Continuous source order reported separately.',
        human_listening='pending',selected=False)


def recount(request,source):
    validate_request(request,source)
    prior=read('prior-analysis.json')
    checked('recording.wav',prior['recording'])
    checked('request.json',prior['request'])
    revised=dict(prior)
    revised.update(line_report(request['canonical_lyrics'],prior['passes']))
    revised['previous_analysis']=descriptor('prior-analysis.json')
    write('analysis.json',revised)
    write('recount.json',dict(schema='reel.lyric-recount-receipt.v1',
        prior=descriptor('prior-analysis.json'),revised=descriptor('analysis.json'),
        recording=descriptor('recording.wav'),source=descriptor(source),
        new_recording=False,new_recognition=False,source_words_changed=False,
        reason='Correct line-presence accounting across repeats; unchanged one-word threshold; continuous-order result retained separately'))
    print('RECOUNT',revised['passed'],revised['qualifying_lines'],'/',revised['total_lines'],
        'continuous-order',revised['continuous_order_qualifying_lines'],flush=True)


def analyze(request, runtime, source):
    validate_request(request, source)
    runtime_files(runtime, 'analysis_files')
    checked(runtime['ffmpeg']['path'], runtime['ffmpeg'])
    import numpy as np
    import soundfile as sf
    import torch
    torch.set_num_threads(4)
    samples, rate = sf.read('recording.wav', dtype='float32', always_2d=True)
    seconds = len(samples)/rate
    peak = float(np.max(np.abs(samples)))
    rms = float(np.sqrt(np.mean(np.square(samples, dtype=np.float64))))
    if not np.isfinite(samples).all() or abs(seconds-request['duration_seconds']) > .15 or not .001 < peak <= 1.001 or rms <= .0001:
        raise ValueError('Recording failed complete decode/duration/finite-signal checks')
    ffmpeg = runtime['ffmpeg']['path']
    subprocess.run([ffmpeg, '-v', 'error', '-i', 'recording.wav', '-c:a', 'libmp3lame', '-b:a', '256k', 'preview.mp3'], check=True)
    def decode(path, sr=16000):
        return np.frombuffer(subprocess.check_output([ffmpeg, '-v','error','-i',str(path),'-ac','1','-ar',str(sr),'-f','f32le','-']), dtype=np.float32)
    from demucs.states import load_model
    from demucs.apply import apply_model
    print('Estimate vocals with explicit cached Demucs model', flush=True)
    torch.manual_seed(0)
    model = load_model(torch.load(runtime['demucs_path'], map_location='cpu', weights_only=False)).eval()
    raw = subprocess.check_output([ffmpeg,'-v','error','-i','recording.wav','-ac','2','-ar',str(model.samplerate),'-f','f32le','-'])
    mix = torch.from_numpy(np.frombuffer(raw,dtype=np.float32).reshape(-1,2).T.copy())
    ref = mix.mean(0)
    mean, std = ref.mean(), ref.std()
    if std < 1e-8:
        raise ValueError('Silent input')
    with torch.no_grad():
        estimates = apply_model(model, ((mix-mean)/std)[None], device='cuda', shifts=0,
            split=True, segment=7.8, overlap=.25, progress=False, num_workers=0)[0].cpu()
    estimates = estimates*std + mean/len(model.sources)
    vocal_samples = estimates[model.sources.index('vocals')].numpy().T
    if not np.isfinite(vocal_samples).all():
        raise ValueError('Nonfinite vocal estimate')
    sf.write('vocals.wav', vocal_samples, model.samplerate, subtype='FLOAT')
    del model, estimates, mix
    torch.cuda.empty_cache()
    import transformers.pipelines.automatic_speech_recognition as asrm
    asrm.is_torchcodec_available = lambda: False
    from transformers import pipeline
    print('Load explicit offline Whisper; no canonical prompt', flush=True)
    recognition_dtype=request.get('recognition_dtype','float32')
    asr = pipeline('automatic-speech-recognition', model=runtime['whisper_root'], device=0,
        torch_dtype=getattr(torch,recognition_dtype), batch_size=ASR_BATCH_SIZE)
    mixed, vocals = decode('recording.wav'), decode('vocals.wav')
    energy = np.array([np.sqrt(np.mean(vocals[i:i+8000]**2)) for i in range(0,len(vocals),8000)])
    active = np.flatnonzero(energy > max(.0001,float(energy.max())*.04))
    start = max(0,int(active[0]*8000)-24000) if len(active) else 0
    end = min(len(vocals),int((active[-1]+1)*8000)+24000) if len(active) else len(vocals)
    passes = []
    for name, chunk, stride, audio, offset in [('mix30',30,5,mixed,0),('mix20',20,3,mixed,0),('vocals20',20,3,vocals[start:end],start/16000)]:
        print('Transcribe', name, flush=True)
        raw = asr(dict(raw=audio, sampling_rate=16000), chunk_length_s=chunk, stride_length_s=stride,
            return_timestamps='word', generate_kwargs=dict(language='spanish',task='transcribe',num_beams=3,
            condition_on_prev_tokens=False,max_new_tokens=192,repetition_penalty=1.1))
        tokens = []
        for row in raw.get('chunks', []):
            row['timestamp'] = [t+offset if t is not None else None for t in row['timestamp']]
            tokens.extend(dict(word=w,span=row['timestamp']) for w in words(row['text']))
        write(Path('transcripts')/(name+'.json'), dict(raw,source_clock_offset_seconds=offset,canonical_prompt=False,
            batch_size=ASR_BATCH_SIZE,dtype=recognition_dtype,chunk_length_s=chunk,stride_length_s=stride))
        passes.append(dict(name=name, text=raw['text'], tokens=tokens))
    report = line_report(request['canonical_lyrics'], passes)
    report.update(schema='reel.song-lyric-diagnostic.v1',candidate_id=request['candidate_id'],
        request=descriptor('request.json'),recording=descriptor('recording.wav'),runtime=descriptor('runtime.json'),
        technical=dict(seconds=seconds,sample_rate=rate,channels=samples.shape[1],peak=peak,rms=rms,complete_decode=True),
        vocal_estimate='Demucs derivative; not an original stem',passes=passes)
    report['recognition_settings']=dict(batch_size=ASR_BATCH_SIZE,dtype=recognition_dtype,num_beams=3,
        conditions=['mix30/5','mix20/3','vocals20/3'],canonical_prompt=False)
    report['requested_complete_poem_passes']=None if request.get('repeat_section_lines') else request.get('performance_repetitions',1)
    report['requested_section_repeat_lines']=request.get('repeat_section_lines')
    write('analysis.json', report)
    with zipfile.ZipFile('transcripts.zip','x',zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(Path('transcripts').glob('*.json')):
            archive.write(path,path.name)
    print('LYRIC GATE',report['passed'],report['qualifying_lines'],'/',report['total_lines'],
        'exact',report['exact_lines'],'one-word gaps',report['one_word_gap_lines'],flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation',choices=['plan','generate','analyze','recount'])
    parser.add_argument('--request',default='request.json')
    parser.add_argument('--runtime',default='runtime.json')
    parser.add_argument('--source',default='source.txt')
    args = parser.parse_args()
    offline()
    request, runtime, source = read(args.request), read(args.runtime), Path(args.source)
    checked(source, request['source'])
    if args.operation=='recount':recount(request,source)
    else:{'plan':plan,'generate':generate,'analyze':analyze}[args.operation](request,runtime,source)


if __name__ == '__main__':
    main()
