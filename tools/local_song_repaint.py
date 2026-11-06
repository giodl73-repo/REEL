"""Bounded offline repaint patches with sample-exact preservation outside masks.

Project-specific text, timing and owner authority are frozen job inputs.
"""
import os
from pathlib import Path
import subprocess
import sys
import re
import json

sys.dont_write_bytecode = True

from local_song_backend import checked, descriptor, offline, read, runtime_files, write


def validate(plan, frames, rate):
    if plan['schema'] != 'reel.local-song-repaint.v1':
        raise ValueError('Unsupported repaint plan')
    if not 1 <= len(plan['patches']) <= 3:
        raise ValueError('Expected one to three bounded patches')
    if type(plan.get('instrumental', False)) is not bool:
        raise ValueError('Instrumental repaint mode must be boolean')
    if plan.get('repaint_mode', 'aggressive') not in ['conservative', 'balanced', 'aggressive']:
        raise ValueError('Unsupported repaint mode')
    strength = plan.get('repaint_strength', .5)
    if isinstance(strength, bool) or not isinstance(strength, (int, float)) or not 0 <= strength <= 1:
        raise ValueError('Repaint strength must be between zero and one')
    previous = 0
    for patch in plan['patches']:
        start, end = patch['start'], patch['end']
        if not previous <= start < end <= frames / rate or end-start > 30:
            raise ValueError('Invalid overlapping or oversized patch')
        if not patch['lyrics'].strip():
            raise ValueError('Patch lyrics required')
        if plan.get('instrumental', False) and patch['lyrics'] != '[Instrumental]':
            raise ValueError('Instrumental repaint requires the exact instrumental marker')
        previous = end
    insertion = plan.get('insertion')
    if insertion:
        if not 0 <= insertion['at'] <= frames/rate or not 0 < insertion['seconds'] <= 12:
            raise ValueError('Invalid bounded insertion')
        if not any(p['start'] <= insertion['at'] and insertion['at']+insertion['seconds'] <= p['end'] for p in plan['patches']):
            raise ValueError('Insertion must be fully inside a repaint mask')


def validate_source_mode(plan, request):
    instrumental = plan.get('instrumental', False)
    if instrumental != request.get('instrumental', False):
        raise ValueError('Repaint mode differs from source performance')
    if instrumental:
        permissions=dict(private_only=True, external_upload=False, identifiable_imitation=False, public_release=False)
        if request.get('canonical_lyrics') != '' or request.get('render_lyrics') != '[Instrumental]':
            raise ValueError('Instrumental source contains lyrics')
        if request.get('permissions') != permissions or plan.get('permissions') != permissions:
            raise ValueError('Instrumental repaint requires exact private permission scope')


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


class OriginalRepairProgress:
    """Bounded, synced phase markers only; no source text or process log."""
    stages = frozenset(('entered','controls-validated','soundfile-import-start',
        'soundfile-import-complete','source-info-start','source-validated',
        'runtime-verify-start','runtime-verified','torch-import-start',
        'torch-import-complete','ace-import-start','ace-import-complete',
        'model-init-start','model-init-complete','runtime-check-complete',
        'patch-start','patch-complete','generation-complete','python-exception'))

    def __init__(self, path='provider-progress.jsonl'):
        self.stream = Path(path).open('x', encoding='utf-8', newline='\n')
        self.failure_path = Path(path).with_name('provider-outcome.json')
        self.count, self.bytes = 0, 0

    def __enter__(self):
        return self

    def emit(self, stage, patch=None):
        if stage not in self.stages or (patch is not None and (type(patch) is not int or not 0 <= patch <= 2)):
            raise ValueError('Unsupported provider progress marker')
        value = dict(schema='reel.original-song-provider-progress.v1',stage=stage)
        if patch is not None:
            value['patch'] = patch
        line = json.dumps(value, separators=(',',':'))+'\n'
        length = len(line.encode('utf-8'))
        if self.count >= 32 or self.bytes+length > 16384:
            raise ValueError('Provider progress exceeds bound')
        self.stream.write(line)
        self.stream.flush()
        os.fsync(self.stream.fileno())
        self.count += 1
        self.bytes += length

    def __exit__(self, kind, value, traceback):
        try:
            # Emit on success too: managed jobs require every declared output.
            # Never replace existing evidence or hide the original exception.
            try:
                message = str(value).encode('utf-8', errors='replace') if kind is not None else b''
                outcome = dict(schema='reel.original-song-provider-outcome.v1',
                    failed=kind is not None,
                    exception_type=kind.__name__[:128] if kind is not None else None,
                    message=message[:4096].decode('utf-8', errors='ignore') if kind is not None else None,
                    message_truncated=len(message)>4096,
                    private_only=True,shareable=False,human_approval=False)
                raw = json.dumps(outcome, ensure_ascii=False).encode('utf-8')
                if len(raw) <= 32768:
                    with self.failure_path.open('xb') as stream:
                        stream.write(raw)
                        stream.flush()
                        os.fsync(stream.fileno())
            except Exception:
                pass
            if kind is not None:
                try:
                    self.emit('python-exception')
                except (ValueError, OSError):
                    pass
        finally:
            self.stream.close()
        return False


def validate_runtime_controls(model_precision, offload_dit_to_cpu, quantization, compute_device):
    if model_precision not in ('float32','float16','bfloat16') or type(offload_dit_to_cpu) is not bool:
        raise ValueError('Invalid explicit precision or diffusion CPU offload')
    if quantization not in ('none','w8a8_dynamic') or compute_device not in ('cpu','cuda'):
        raise ValueError('Unsupported explicit quantization or compute device')
    if compute_device == 'cpu' and (model_precision != 'float32' or quantization != 'none' or offload_dit_to_cpu):
        raise ValueError('CPU route requires float32, no quantization and no diffusion offload')


def initialize_ace(runtime, progress=None, model_precision='float32', offload_dit_to_cpu=False, quantization='w8a8_dynamic', compute_device='cuda'):
    note = progress.emit if progress is not None else lambda stage: None
    validate_runtime_controls(model_precision,offload_dit_to_cpu,quantization,compute_device)
    os.environ['ACESTEP_ROCM_DTYPE'] = model_precision
    sys.path.insert(0, runtime['ace_root'])
    note('torch-import-start')
    import torch
    note('torch-import-complete')
    note('ace-import-start')
    import acestep.gpu_config as gpu_config
    if model_precision == 'float32' and compute_device == 'cuda':
        gpu_config.is_rocm_available = lambda: True
    from acestep.handler import AceStepHandler
    from acestep.inference import GenerationConfig, GenerationParams, generate_music
    note('ace-import-complete')
    torch.set_num_threads(4)
    handler = AceStepHandler()
    note('model-init-start')
    original_bfloat16_probe = torch.cuda.is_bf16_supported
    try:
        # An explicit float16 request must not select emulated/default bfloat16.
        # This changes precision policy only inside this initialization call.
        if model_precision == 'float16':
            torch.cuda.is_bf16_supported = lambda *args, **kwargs: False
        status, ok = handler.initialize_service(project_root=runtime['ace_root'],config_path='acestep-v15-turbo',device=compute_device,use_flash_attention=False,compile_model=False,offload_to_cpu=True,offload_dit_to_cpu=offload_dit_to_cpu,quantization=None if quantization == 'none' else quantization,use_mlx_dit=False)
    finally:
        torch.cuda.is_bf16_supported = original_bfloat16_probe
    expected_dtype = getattr(torch,model_precision)
    if not ok:
        raise RuntimeError(str(status))
    if handler.dtype is not expected_dtype:
        raise RuntimeError('Initialized model dtype differs from explicitly requested precision')
    if offload_dit_to_cpu and getattr(handler,'offload_dit_to_cpu',False) is not True:
        raise RuntimeError('Initialized model did not enable requested diffusion CPU offload')
    if quantization == 'none' and getattr(handler,'quantization','missing') is not None:
        raise RuntimeError('Initialized model did not disable requested quantization')
    if str(getattr(handler,'device','missing')) != compute_device:
        raise RuntimeError('Initialized model device differs from explicitly requested compute device')
    note('model-init-complete')
    return handler, GenerationParams, GenerationConfig, generate_music


def validate_original_repair(request, context, rate, frames, channels):
    """Separate native-context route; never accepts an older modern guide request."""
    scope = dict(private_only=True, external_upload=False, identifiable_imitation=False, public_release=False)
    if not 8000 <= rate <= 96000 or channels not in (1,2) or not 0 < frames <= rate*600:
        raise ValueError('Unsupported original native clock')
    if request.get('schema') != 'reel.original-song-repair-provider-request.v1' or context.get('schema') != 'reel.song-repair-source-context.v1':
        raise ValueError('Unsupported original-source repair context')
    for flags in [request.get('permissions'), context.get('private_scope')]:
        if not isinstance(flags, dict) or set(flags) != set(scope) or any(type(flags[k]) is not bool or flags[k] != v for k,v in scope.items()):
            raise ValueError('Original-source private restrictions differ')
    for field in ['original_request_rewritten','provider_executed','human_approval','shareable','mapping_authority_verified']:
        if context.get(field) is not False:
            raise ValueError('Original-source context exceeds its authority')
    if request.get('attempts_per_patch') != 1 or type(request.get('attempts_per_patch')) is not int or request.get('semantic_planner') is not False:
        raise ValueError('Only one nonsemantic provider attempt per patch is admitted')
    if request.get('source') != context.get('recording'):
        raise ValueError('Original recording context differs')
    for key, actual in [('sample_rate',rate),('source_frames',frames),('channels',channels)]:
        if type(request.get(key)) is not int or request[key] != actual or context.get(key) != actual:
            raise ValueError('Original native clock differs')
    seed = request.get('seed')
    if type(seed) is not int or not 0 <= seed <= 2**32-4:
        raise ValueError('Explicit bounded seed required')
    strength = request.get('repaint_strength')
    if type(strength) not in (int,float) or not 0 <= strength <= 1 or request.get('repaint_mode') not in ['conservative','balanced','aggressive']:
        raise ValueError('Explicit repaint controls required')
    validate_runtime_controls(request.get('model_precision','float32'),request.get('offload_dit_to_cpu',False),request.get('quantization','w8a8_dynamic'),request.get('compute_device','cuda'))
    if not isinstance(request.get('caption'),str) or not request['caption'].strip() or len(request['caption'].encode('utf-8')) > 4096:
        raise ValueError('Bounded private provider caption required')
    patches = request.get('patches')
    if not isinstance(patches,list) or not 1 <= len(patches) <= 3:
        raise ValueError('Bounded original mask roster required')
    text = context.get('canonical_lyrics')
    if not isinstance(text,str):
        raise ValueError('Original canonical source missing')
    lines = [line for line in re.split('[\n\r\v\f\x1c\x1d\x1e\x85\u2028\u2029]',text) if line.strip()]
    previous = 0
    for patch in patches:
        for key in ['start_frame','end_frame','fade_frames','context_start_frame','context_end_frame']:
            if type(patch.get(key)) is not int:
                raise ValueError('Integer native mask geometry required')
        start,end = patch['start_frame'],patch['end_frame']
        if not previous <= start < end <= frames or end-start > 30*rate or not 0 <= patch['fade_frames'] <= min(rate,(end-start-1)//2):
            raise ValueError('Original mask or fades differ')
        if patch['context_start_frame'] != max(0,start-5*rate) or patch['context_end_frame'] != min(frames,end+5*rate):
            raise ValueError('Original neighboring music context differs')
        selected = patch.get('source_lines')
        if not isinstance(selected,list) or len(selected) != 2 or any(type(n) is not int for n in selected):
            raise ValueError('Canonical line range required')
        first,last = selected
        if not 1 <= first <= last <= len(lines) or patch.get('supplied_lyrics') != '\n'.join(lines[first-1:last]):
            raise ValueError('Supplied words differ from exact canonical lines')
        previous = end


def original_repair_main(runtime_check_only=False):
    """Raw candidates only. Rust owns original correspondence and materialization."""
    with OriginalRepairProgress() as progress:
        progress.emit('entered')
        return _original_repair_main(progress, runtime_check_only)


def _original_repair_main(progress, runtime_check_only):
    offline()
    request, context, runtime = read('provider-request.json'), read('source-context.json'), read('runtime.json')
    checked('source-context.json',request['source_context'])
    checked('runtime.json',request['runtime'])
    checked('source.wav',request['source'])
    checked(Path(__file__),request['backend'])
    checked(Path(__file__).with_name('local_song_backend.py'),request['backend_helper'])
    progress.emit('controls-validated')
    progress.emit('soundfile-import-start')
    import soundfile as sf
    progress.emit('soundfile-import-complete')
    progress.emit('source-info-start')
    info = sf.info('source.wav')
    if info.subtype != 'FLOAT':
        raise ValueError('Original repair requires a float32 source')
    validate_original_repair(request,context,info.samplerate,info.frames,info.channels)
    for index, patch in enumerate(request['patches']):
        path = f'patch-source-{index}.wav'
        checked(path,patch['context_audio'])
        window = sf.info(path)
        if window.samplerate != info.samplerate or window.channels != info.channels or window.frames != patch['context_end_frame']-patch['context_start_frame'] or window.subtype != 'FLOAT':
            raise ValueError('Native prepared conditioning window differs')
    progress.emit('source-validated')
    progress.emit('runtime-verify-start')
    runtime_files(runtime,'generation_files')
    checked(runtime['python']['path'],runtime['python'])
    if subprocess.check_output(['git','-C',runtime['ace_root'],'rev-parse','HEAD'],text=True).strip() != runtime['ace_commit']:
        raise ValueError('Original repair provider source changed')
    progress.emit('runtime-verified')
    handler, Params, Config, generate = initialize_ace(runtime, progress, request.get('model_precision','float32'),request.get('offload_dit_to_cpu',False),request.get('quantization','w8a8_dynamic'),request.get('compute_device','cuda'))
    if runtime_check_only:
        receipt = dict(schema='reel.original-song-provider-runtime-check.v1',
            request=descriptor('provider-request.json'),runtime=descriptor('runtime.json'),source_context=descriptor('source-context.json'),
            model_initialized=True,provider_generation_called=False,human_approval=False,selected=False)
        for key in ('model_precision','offload_dit_to_cpu','quantization','compute_device'):
            if key in request:
                receipt[key] = request[key]
        write('provider-runtime-check.json',receipt)
        progress.emit('runtime-check-complete')
        return
    attempts = []
    for index, patch in enumerate(request['patches']):
        start = patch['context_start_frame']
        params = Params(task_type='repaint',src_audio=f'patch-source-{index}.wav',
            repainting_start=(patch['start_frame']-start)/info.samplerate,
            repainting_end=(patch['end_frame']-start)/info.samplerate,
            chunk_mask_mode='explicit',repaint_mode=request['repaint_mode'],repaint_strength=request['repaint_strength'],repaint_wav_crossfade_sec=0,
            caption=request['caption'],lyrics=patch['supplied_lyrics'],instrumental=False,vocal_language='es',
            duration=(patch['context_end_frame']-start)/info.samplerate,inference_steps=8,guidance_scale=7.5,
            seed=request['seed']+index,thinking=False,use_cot_metas=False,use_cot_caption=False,use_cot_language=False,use_cot_lyrics=False)
        progress.emit('patch-start',index)
        result = generate(handler,None,params,Config(batch_size=1,use_random_seed=False,seeds=[params.seed],audio_format='wav'),save_dir=f'generated-{index}')
        if not result.success or len(result.audios) != 1:
            raise RuntimeError(str(result.error))
        destination = f'raw-patch-{index}.wav'
        os.link(result.audios[0]['path'],destination)
        attempts.append(dict(index=index,attempt=1,seed=params.seed,context_start_frame=start,
            context_end_frame=patch['context_end_frame'],context_audio=patch['context_audio'],raw_audio=descriptor(destination)))
        progress.emit('patch-complete',index)
    write('provider-receipt.json',dict(schema='reel.original-song-repair-provider-receipt.v1',
        request=descriptor('provider-request.json'),runtime=descriptor('runtime.json'),source_context=descriptor('source-context.json'),
        patches=attempts,external_upload=False,human_approval=False,selected=False,performed_words_verified=False))
    progress.emit('generation-complete')


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
    validate_source_mode(plan, request)
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
    handler, GenerationParams, GenerationConfig, generate_music = initialize_ace(runtime)
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
        params=GenerationParams(task_type='repaint',src_audio=source,repainting_start=patch['start']-offset,repainting_end=patch['end']-offset,chunk_mask_mode='explicit',repaint_mode=plan.get('repaint_mode','aggressive'),repaint_strength=plan.get('repaint_strength',.5),repaint_wav_crossfade_sec=0,caption=plan['caption'],lyrics=patch['lyrics'],instrumental=plan.get('instrumental',False),vocal_language='es',duration=len(context)/rate,inference_steps=8,guidance_scale=7.5,seed=plan['seed']+i,thinking=False,use_cot_metas=False,use_cot_caption=False,use_cot_language=False,use_cot_lyrics=False)
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
    retained, retained_rate = sf.read('recording.wav', dtype='float32', always_2d=True)
    if retained_rate != rate or not np.array_equal(retained, working):
        raise ValueError('Retained float WAV differs from verified repaint samples')
    write('repaint-receipt.json',dict(schema='reel.local-song-repaint-receipt.v1',plan=descriptor('repaint.json'),source=descriptor('source.wav'),recording=descriptor('recording.wav'),patches=results,insertion=insertion,instrumental=plan.get('instrumental',False),unmasked_samples_exact=True,duration_seconds=len(working)/rate,semantic_planner=False,external_upload=False,human_listening='pending',selected=False))
    revised=dict(request,candidate_id=plan['candidate_id'],duration_seconds=len(working)/rate,max_duration_seconds=len(working)/rate)
    write('request.json',revised)
    if plan.get('make_listening_preview') is True:
        ffmpeg=runtime['ffmpeg'];checked(ffmpeg['path'],ffmpeg)
        subprocess.run([ffmpeg['path'],'-v','error','-i','recording.wav','-c:a','libmp3lame','-b:a','256k','preview.mp3'],check=True)
        decoded=np.frombuffer(subprocess.check_output([ffmpeg['path'],'-v','error','-i','preview.mp3','-ac','1','-ar','16000','-f','f32le','-']),dtype=np.float32)
        if not np.isfinite(decoded).all() or abs(len(decoded)/16000-len(working)/rate)>.05:
            raise ValueError('Listening preview failed full decode or duration check')
        write('listening-check.json',dict(recording=descriptor('recording.wav'),listening=descriptor('preview.mp3'),full_decode=True,duration_seconds=len(decoded)/16000,unmasked_lossless_samples_exact=True,lossless_write_read_exact=True,protected_frames=int(mask.sum()),changed_mask_frames=int((~mask).sum()),preview_encoding='256kbps MP3, no EQ or pitch filter; lossy preview is re-encoded',human_listening='pending',selected=False))


if __name__=='__main__':
    if sys.argv[1:] == ['--original-repair']:
        original_repair_main()
    elif sys.argv[1:] == ['--original-repair-runtime-check']:
        original_repair_main(runtime_check_only=True)
    elif not sys.argv[1:]:
        main()
    else:
        raise ValueError('Unsupported repaint invocation')
