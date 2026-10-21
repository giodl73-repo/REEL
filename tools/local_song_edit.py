"""Bounded sample-preserving song editions with source-derived word evidence.

Runs only as an explicit trusted Rust job. It does not generate a performance,
recognize new words, change tempo/pitch or grant a creative selection.
"""
import math
from pathlib import Path
import local_song_backend as backend


def segment_plan(segments, rate, frames, crossfade, fade_in, fade_out, ceiling):
    if not isinstance(segments,list) or not 1<=len(segments)<=64:
        raise ValueError('Edition requires 1-64 explicit source segments')
    values=[crossfade,fade_in,fade_out,ceiling]
    if any(type(x) not in (int,float) or not math.isfinite(x) for x in values):
        raise ValueError('Edition bounds must be finite numbers')
    if not 0<=crossfade<=1 or not 0<=fade_in<=5 or not 0<=fade_out<=5 or not 0<ceiling<=600:
        raise ValueError('Unsupported edition fade/duration bounds')
    rows=[];cursor=0;previous_end=0
    for i,pair in enumerate(segments):
        if not isinstance(pair,list) or len(pair)!=2 or any(type(x) not in (int,float) or not math.isfinite(x) for x in pair):
            raise ValueError('Invalid source segment')
        start,end=[round(x*rate) for x in pair]
        if start<previous_end or not 0<=start<end<=frames:
            raise ValueError('Source segments must be ordered, nonoverlapping and in bounds')
        blend=round(crossfade*rate) if i else 0
        left=max(blend,round(fade_in*rate) if i==0 else 0)
        right=max(round(crossfade*rate) if i<len(segments)-1 else 0,round(fade_out*rate) if i==len(segments)-1 else 0)
        if end-start<=left+right:
            raise ValueError('Fades consume a source segment')
        output_start=cursor-blend
        rows.append(dict(source_start=start,source_end=end,output_start=output_start,
            core_start=start+left,core_end=end-right,blend_frames=blend))
        cursor=output_start+end-start;previous_end=end
    if cursor/rate>ceiling or cursor<=0:
        raise ValueError('Edition exceeds its hard duration ceiling')
    return rows,cursor


def remap_passes(passes,plan,rate):
    result=[]
    for original in passes:
        tokens=[]
        for segment in plan:
            lo,hi=segment['core_start']/rate,segment['core_end']/rate
            offset=(segment['output_start']-segment['source_start'])/rate
            for token in original['tokens']:
                span=token.get('span')
                if span and len(span)==2 and all(type(x) in (int,float) and math.isfinite(x) for x in span) and lo<=span[0]<=span[1]<=hi:
                    tokens.append(dict(token,span=[span[0]+offset,span[1]+offset]))
        result.append(dict(name='source-'+original['name']+'-unchanged-samples',tokens=tokens,
            text=' '.join(x['word'] for x in tokens),source_pass=original['name'],
            canonical_prompt=False,new_recognition=False))
    return result


def main():
    recipe=backend.read('edit.json')
    if recipe.get('schema')!='reel.song-short-edit.v1' or recipe.get('task')!='short-edit':
        raise ValueError('Unsupported edition recipe')
    for path,key in [('source.wav','source_recording'),('source-request.json','source_request'),
                     ('source-analysis.json','source_analysis'),('source-generation.json','source_generation')]:
        backend.checked(path,recipe[key])
    original=backend.read('source-request.json');prior=backend.read('source-analysis.json');generation=backend.read('source-generation.json')
    backend.checked('source.txt',original['source']);backend.validate_request(original,Path('source.txt'))
    if original['source']!=recipe['source'] or original['source_lock']!=recipe['source_lock']:
        raise ValueError('Edition source/version differs from the original')
    if prior['recording']!=backend.descriptor('source.wav') or prior['request']!=backend.descriptor('source-request.json'):
        raise ValueError('Original recognition is not bound to the source recording/request')
    if generation['recording']!=backend.descriptor('source.wav') or generation['request']!=backend.descriptor('source-request.json'):
        raise ValueError('Original generated recording proof differs')
    if original['permissions']!=recipe['permissions']:
        raise ValueError('Edition cannot expand source permissions')
    if [x['canonical'] for x in prior['lines']]!=[x for x in original['canonical_lyrics'].splitlines() if x.strip()]:
        raise ValueError('Original recognition has different canonical lines')
    if recipe['caption']!=original['caption'] or recipe['source_generation_duration_seconds']!=original['duration_seconds']:
        raise ValueError('Edition must preserve the actual original generation brief')
    import numpy as np
    import soundfile as sf
    info=sf.info('source.wav')
    if not 1<=info.channels<=2 or not 8000<=info.samplerate<=96000 or info.duration>600:
        raise ValueError('Source exceeds bounded audio intake')
    samples,rate=sf.read('source.wav',dtype='float32',always_2d=True)
    if not np.isfinite(samples).all():raise ValueError('Nonfinite source samples')
    plan,frames=segment_plan(recipe['segments'],rate,len(samples),recipe['crossfade_seconds'],recipe['fade_in_seconds'],recipe['fade_out_seconds'],recipe['max_duration_seconds'])
    if abs(recipe['duration_seconds']-frames/rate)>1/rate:
        raise ValueError('Declared edition duration differs from its sample plan')
    output=samples[plan[0]['source_start']:plan[0]['source_end']].copy()
    for segment in plan[1:]:
        part=samples[segment['source_start']:segment['source_end']].copy();n=segment['blend_frames']
        if n:
            weight=np.linspace(0,1,n,dtype=np.float32)[:,None]
            output[-n:]=output[-n:]*(1-weight)+part[:n]*weight
            output=np.concatenate([output,part[n:]])
        else:output=np.concatenate([output,part])
    for count,at_end in [(round(recipe['fade_in_seconds']*rate),False),(round(recipe['fade_out_seconds']*rate),True)]:
        if count:
            ramp=np.linspace(0,1,count,dtype=np.float32)[:,None]
            if at_end:output[-count:]*=ramp[::-1]
            else:output[:count]*=ramp
    if len(output)!=frames or not np.isfinite(output).all():raise ValueError('Invalid rendered edition')
    sf.write('recording.wav',output,rate,subtype='FLOAT')
    decoded,check_rate=sf.read('recording.wav',dtype='float32',always_2d=True)
    if check_rate!=rate:raise ValueError('Edition sample rate changed')
    for segment in plan:
        a,b=segment['core_start'],segment['core_end'];out=segment['output_start']+a-segment['source_start']
        if not np.array_equal(samples[a:b],decoded[out:out+b-a]):
            raise ValueError('Evidence-bearing source samples changed')
    passes=remap_passes(prior['passes'],plan,rate)
    report=backend.line_report(original['canonical_lyrics'],passes)
    peak=float(np.max(np.abs(decoded)));rms=float(np.sqrt(np.mean(decoded.astype(np.float64)**2)))
    if not .001<peak<=1.001 or rms<=.0001:raise ValueError('Edition failed signal checks')
    report.update(schema='reel.song-lyric-diagnostic.v1',candidate_id=recipe['candidate_id'],request=backend.descriptor('edit.json'),
        recording=backend.descriptor('recording.wav'),analysis_kind='source-derived-recognition-on-exact-unchanged-samples',
        source_recording=backend.descriptor('source.wav'),source_analysis=backend.descriptor('source-analysis.json'),
        passes=passes,new_recognition=False,technical=dict(seconds=len(decoded)/rate,sample_rate=rate,channels=decoded.shape[1],
        peak=peak,rms=rms,complete_decode=True),
        requested_complete_poem_passes=1,source_sample_equality=True,human_listening='pending')
    backend.write('analysis.json',report)
    backend.write('edit-receipt.json',dict(schema='reel.song-short-edit-receipt.v1',recipe=backend.descriptor('edit.json'),
        source_recording=backend.descriptor('source.wav'),source_request=backend.descriptor('source-request.json'),
        source_analysis=backend.descriptor('source-analysis.json'),source_generation=backend.descriptor('source-generation.json'),
        recording=backend.descriptor('recording.wav'),plan=plan,sample_rate=rate,output_frames=frames,
        evidence_core_sample_equality=True,new_recognition=False,tempo_changed=False,pitch_changed=False,
        transformed_regions='Declared instrumental edge fades only; word evidence excludes every changed edge',
        new_performance=False,new_human_approval=False))
    print('SHORT EDIT',round(len(decoded)/rate,3),'seconds; gate',report['qualifying_lines'],'/',report['total_lines'],flush=True)


if __name__=='__main__':main()
