"""Bounded offline repaint patches with sample-exact preservation outside masks.

Project-specific text, timing and owner authority are frozen job inputs.
"""
import os
from pathlib import Path
import subprocess
import sys

from local_song_backend import checked, descriptor, offline, read, runtime_files, write


def validate(plan, frames, rate):
    if plan['schema'] != 'reel.local-song-repaint.v1':
        raise ValueError('Unsupported repaint plan')
    if not 1 <= len(plan['patches']) <= 3:
        raise ValueError('Expected one to three bounded patches')
    previous = 0
    for patch in plan['patches']:
        start, end = patch['start'], patch['end']
        if not previous <= start < end <= frames / rate or end-start > 30:
            raise ValueError('Invalid overlapping or oversized patch')
        if not patch['lyrics'].strip():
            raise ValueError('Patch lyrics required')
        previous = end
    insertion = plan.get('insertion')
    if insertion:
        if not 0 <= insertion['at'] <= frames/rate or not 0 < insertion['seconds'] <= 12:
            raise ValueError('Invalid bounded insertion')
        if not any(p['start'] <= insertion['at'] and insertion['at']+insertion['seconds'] <= p['end'] for p in plan['patches']):
            raise ValueError('Insertion must be fully inside a repaint mask')


def splice(original, generated, rate, start, end, fade_seconds=.12):
    import numpy as np
    if original.shape != generated.shape:
        raise ValueError('Repaint changed audio dimensions')
    a, b = round(start*rate), round(end*rate)
    n = min(round(fade_seconds*rate), (b-a)//2)
    result = original.copy()
    result[a:b] = generated[a:b]
    if n:
        gain = np.linspace(0, 1, n, dtype=np.float32)[:, None]
        result[a:a+n] = original[a:a+n]*(1-gain)+generated[a:a+n]*gain
        result[b-n:b] = generated[b-n:b]*(1-gain)+original[b-n:b]*gain
    if not np.array_equal(result[:a], original[:a]) or not np.array_equal(result[b:], original[b:]):
        raise ValueError('Unmasked samples changed')
    return result


def main():
    offline()
    plan, runtime, request = read('repaint.json'), read('runtime.json'), read('source-request.json')
    checked('source.wav', plan['recording'])
    checked('source-request.json', plan['request'])
    checked('source.txt', plan['source'])
    if Path('source.txt').read_text(encoding='utf-8') != request['canonical_lyrics']:
        raise ValueError('Source lyric binding differs')
    if not request['permissions']['private_only'] or request['permissions']['external_upload']:
        raise ValueError('Only private offline sources supported')
    runtime_files(runtime, 'generation_files')
    checked(runtime['python']['path'],runtime['python'])
    actual = subprocess.check_output(['git','-C',runtime['ace_root'],'rev-parse','HEAD'],text=True).strip()
    if actual != runtime['ace_commit']:
        raise ValueError('ACE source changed')
    import numpy as np
    import soundfile as sf
    original, rate = sf.read('source.wav', dtype='float32', always_2d=True)
    validate(plan,len(original)+round(plan.get('insertion',{}).get('seconds',0)*rate),rate)
    working = original.copy()
    insertion = plan.get('insertion')
    if insertion:
        at, count = round(insertion['at']*rate), round(insertion['seconds']*rate)
        working = np.concatenate([working[:at],np.zeros((count,working.shape[1]),dtype=np.float32),working[at:]])
    baseline = working.copy()
    os.environ['ACESTEP_ROCM_DTYPE']='float32'
    sys.path.insert(0,runtime['ace_root'])
    import torch
    import acestep.gpu_config as gpu_config
    gpu_config.is_rocm_available=lambda: True
    from acestep.handler import AceStepHandler
    from acestep.inference import GenerationConfig, GenerationParams, generate_music
    torch.set_num_threads(4)
    handler=AceStepHandler()
    status,ok=handler.initialize_service(project_root=runtime['ace_root'],config_path='acestep-v15-turbo',device='cuda',use_flash_attention=False,compile_model=False,offload_to_cpu=True,offload_dit_to_cpu=False,quantization='w8a8_dynamic',use_mlx_dit=False)
    if not ok or handler.dtype is not torch.float32:
        raise RuntimeError(str(status))
    results=[]
    for i,patch in enumerate(plan['patches']):
        source=f'patch-source-{i}.wav'
        # Bound source encoding to the patch plus actual neighboring music.
        # The full master never passes through a generative reconstruction.
        context_start=max(0,round((patch['start']-5)*rate))
        context_end=min(len(working),round((patch['end']+5)*rate))
        context=working[context_start:context_end]
        offset=context_start/rate
        sf.write(source,context,rate,subtype='FLOAT')
        params=GenerationParams(task_type='repaint',src_audio=source,repainting_start=patch['start']-offset,repainting_end=patch['end']-offset,chunk_mask_mode='explicit',repaint_mode='aggressive',repaint_wav_crossfade_sec=0,caption=plan['caption'],lyrics=patch['lyrics'],instrumental=False,vocal_language='es',duration=len(context)/rate,inference_steps=8,guidance_scale=7.5,seed=plan['seed']+i,thinking=False,use_cot_metas=False,use_cot_caption=False,use_cot_language=False,use_cot_lyrics=False)
        print('Repaint bounded patch',i,patch['start'],patch['end'],flush=True)
        result=generate_music(handler,None,params,GenerationConfig(batch_size=1,use_random_seed=False,seeds=[params.seed],audio_format='wav'),save_dir=f'generated-{i}')
        if not result.success or len(result.audios)!=1:
            raise RuntimeError(str(result.error))
        os.link(result.audios[0]['path'],f'raw-patch-{i}.wav')
        generated,sr=sf.read(result.audios[0]['path'],dtype='float32',always_2d=True)
        if sr!=rate or not np.isfinite(generated).all():
            raise ValueError('Invalid generated audio')
        # ACE's latent grid can add or remove a final partial frame. Require
        # the entire patch and retain the original context outside its mask.
        if abs(len(generated)-len(context))>round(.1*rate):
            raise ValueError('Generated context duration changed materially')
        required=round((patch['end']-offset)*rate)
        if len(generated)<required:
            raise ValueError('Generated patch is incomplete')
        bounded=context.copy()
        count=min(len(generated),len(context))
        bounded[:count]=generated[:count]
        working[context_start:context_end]=splice(context,bounded,rate,patch['start']-offset,patch['end']-offset)
        results.append(dict(patch=patch,context_start=offset,context_end=context_end/rate,raw_generated=descriptor(result.audios[0]['path'])))
    mask=np.ones(len(working),dtype=bool)
    for patch in plan['patches']:mask[round(patch['start']*rate):round(patch['end']*rate)]=False
    if not np.array_equal(working[mask],baseline[mask]):
        raise ValueError('Protected source samples changed')
    if not np.isfinite(working).all() or np.max(np.abs(working))>1.001:
        raise ValueError('Invalid output signal')
    sf.write('recording.wav',working,rate,subtype='FLOAT')
    write('repaint-receipt.json',dict(schema='reel.local-song-repaint-receipt.v1',plan=descriptor('repaint.json'),source=descriptor('source.wav'),recording=descriptor('recording.wav'),patches=results,insertion=insertion,unmasked_samples_exact=True,duration_seconds=len(working)/rate,semantic_planner=False,external_upload=False,human_listening='pending',selected=False))
    revised=dict(request,candidate_id=plan['candidate_id'],duration_seconds=len(working)/rate,max_duration_seconds=len(working)/rate)
    write('request.json',revised)


if __name__=='__main__':main()
