"""Offline vocal estimate from an explicitly pinned full recording and Demucs model.

Run as a native declared-output job. This retains a derivative estimate, never
an original performance stem or a newly approved recording.
"""
import math
import re
import subprocess

from local_song_backend import checked, descriptor, offline, read, write


def validate(q):
    if q['schema'] != 'reel.local-song-vocal-estimate.v1':
        raise ValueError('Unsupported vocal estimate request')
    if q['permissions'] != dict(private_only=True, external_upload=False, public_release=False):
        raise ValueError('Explicit private offline scope required')
    if not math.isfinite(q['duration_seconds']) or not 1 <= q['duration_seconds'] <= 1200:
        raise ValueError('Invalid full recording duration')
    if q['device'] not in ['cpu', 'cuda']:
        raise ValueError('Explicit local inference device required')
    if (not re.fullmatch('[0-9a-f]{64}', q['golden_lock']['manifest_sha256'])
            or not q['golden_lock']['version_id']
            or not q['golden_lock']['logical_id'].startswith('song/')):
        raise ValueError('Exact source Golden lock required')


def normalize(mix):
    ref = mix.mean(0)
    mean, std = ref.mean(), ref.std()
    if not bool(std.isfinite()) or float(std) < 1e-8:
        raise ValueError('Silent or nonfinite source')
    return (mix-mean)/std, mean, std


def restore(estimates, mean, std, source_count):
    if source_count < 1 or estimates.shape[0] != source_count:
        raise ValueError('Model source count differs')
    return estimates*std + mean/source_count


def main():
    offline(); q = read('vocal-request.json'); validate(q)
    checked('original.wav', q['recording'])
    ff = checked(q['ffmpeg']['path'], q['ffmpeg'])
    model_path = checked(q['model']['path'], q['model'])
    for row in q['runtime_files']:
        checked(row['path'], row)
    import importlib.metadata
    for name, version in q['runtime_packages'].items():
        if importlib.metadata.version(name) != version:
            raise ValueError('Installed runtime package changed: '+name)
    import numpy as np
    import soundfile as sf
    import torch
    from demucs.states import load_model
    from demucs.apply import apply_model
    torch.set_num_threads(4); torch.manual_seed(0)
    model = load_model(torch.load(model_path, map_location='cpu', weights_only=False)).eval()
    if 'vocals' not in model.sources:
        raise ValueError('Pinned model has no vocal source')
    raw = subprocess.check_output([str(ff), '-v', 'error', '-nostdin', '-i', 'original.wav',
        '-ac', '2', '-ar', str(model.samplerate), '-f', 'f32le', '-'])
    samples = np.frombuffer(raw, dtype='<f4').reshape(-1, 2)
    frames = round(q['duration_seconds']*model.samplerate)
    if len(samples) != frames or not np.isfinite(samples).all():
        raise ValueError('Full source extent or finite samples differ')
    mix = torch.from_numpy(samples.T.copy()); normalized, mean, std = normalize(mix)
    with torch.no_grad():
        estimates = apply_model(model, normalized[None], device=q['device'], shifts=0,
            split=True, segment=7.8, overlap=.25, progress=False, num_workers=0)[0].cpu()
    estimates = restore(estimates, mean, std, len(model.sources))
    vocals = estimates[model.sources.index('vocals')].numpy().T
    if vocals.shape != (frames, 2) or not np.isfinite(vocals).all():
        raise ValueError('Vocal estimate changed source extent or has nonfinite samples')
    sf.write('vocals.wav', vocals, model.samplerate, subtype='FLOAT')
    reread, rate = sf.read('vocals.wav', dtype='float32', always_2d=True)
    if rate != model.samplerate or not np.array_equal(reread, vocals):
        raise ValueError('Written floating-point estimate differs')
    checked('original.wav', q['recording'])
    write('vocal-evidence.json', dict(schema='reel.local-song-vocal-estimate-check.v1',
        source_golden=q['golden_lock'], recording=q['recording'], model=q['model'],
        vocal_estimate=descriptor('vocals.wav'), source_duration_seconds=q['duration_seconds'],
        runtime_packages=q['runtime_packages'],
        sample_rate=rate, frames=frames, source_unchanged=True, full_decode=True,
        source_retimed=False, source_repitched=False, voice_generated=False,
        estimate_method='Pinned Demucs; seed 0; shifts 0; 7.8 second segments; overlap .25',
        limitations=['Derivative estimate may contain accompaniment bleed or separation artifacts.',
            'This is not an original isolated stem or a new creative approval.'],
        human_musical_approval=False, public_release=False))
    print('FULL SOURCE VOCAL ESTIMATE VERIFIED', frames, flush=True)


if __name__ == '__main__':
    main()
