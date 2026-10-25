"""Explicit bounded, unprompted recognition of unchanged song audio windows.

Run as a trusted Rust job using the already frozen Whisper installation. Each
window/condition stays an independent pass; this neither generates nor edits audio.
"""
import math
import subprocess

from local_song_backend import checked, descriptor, line_report, offline, read, runtime_files, words, write


def validate_windows(ranges, conditions, duration):
    if conditions not in [['mix'], ['vocals'], ['mix', 'vocals']]:
        raise ValueError('Unsupported explicit recognition conditions')
    if not isinstance(ranges, list) or not 1 <= len(ranges) <= 4:
        raise ValueError('Recognition requires one to four bounded windows')
    previous = 0
    total = 0
    for pair in ranges:
        if not isinstance(pair, list) or len(pair) != 2 or any(type(v) not in (int, float) or not math.isfinite(v) for v in pair):
            raise ValueError('Invalid recognition window')
        start, end = pair
        if not previous <= start < end <= duration or end-start > 60:
            raise ValueError('Recognition window is overlapping or out of bounds')
        previous = end
        total += end-start
    if total*len(conditions) > 180:
        raise ValueError('Recognition window budget exceeded')


def main():
    offline()
    cfg=read('windows.json');request=read('request.json');prior=read('prior-analysis.json');runtime=read('runtime.json')
    if cfg.get('schema')!='reel.local-song-asr-windows.v1':raise ValueError('Unsupported window check')
    for path,reference in [('prior-analysis.json',cfg['prior_analysis']),('recording.wav',cfg['recording']),
                           ('vocals.wav',cfg['vocal_estimate']),('source.txt',request['source'])]:
        checked(path,reference)
    if prior['recording']!=descriptor('recording.wav') or prior['request']!=descriptor('request.json'):
        raise ValueError('Original recording/request binding differs')
    checked(runtime['python']['path'],runtime['python']);checked(runtime['ffmpeg']['path'],runtime['ffmpeg'])
    runtime_files(runtime,'analysis_files')
    conditions=cfg['conditions'];ranges=cfg['ranges']
    validate_windows(ranges,conditions,prior['technical']['seconds'])
    import numpy as np
    import torch
    import transformers.pipelines.automatic_speech_recognition as asrm
    from transformers import pipeline
    torch.set_num_threads(4);torch.manual_seed(0)
    asrm.is_torchcodec_available=lambda:False
    asr=pipeline('automatic-speech-recognition',model=runtime['whisper_root'],device=-1,torch_dtype=torch.float32,batch_size=1)
    extras=[];raw=[]
    for i,(start,end) in enumerate(ranges,1):
        for condition in conditions:
            audio='recording.wav' if condition=='mix' else 'vocals.wav'
            data=subprocess.check_output([runtime['ffmpeg']['path'],'-v','error','-ss',str(start),'-t',str(end-start),
                '-i',audio,'-ac','1','-ar','16000','-f','f32le','-'])
            waveform=np.frombuffer(data,dtype=np.float32)
            if not np.isfinite(waveform).all() or abs(len(waveform)/16000-(end-start))>.05:
                raise ValueError('Window decode failed')
            result=asr(dict(raw=waveform,sampling_rate=16000),chunk_length_s=30,stride_length_s=5,
                return_timestamps='word',generate_kwargs=dict(language='spanish',task='transcribe',num_beams=3,
                condition_on_prev_tokens=False,max_new_tokens=192,repetition_penalty=1.1))
            tokens=[]
            for chunk in result.get('chunks',[]):
                span=[t+start if t is not None else None for t in chunk['timestamp']]
                tokens.extend(dict(word=w,span=span) for w in words(chunk['text']))
            name=f'independent-whisper-cpu-window-{i}-{condition}'
            extras.append(dict(name=name,text=result['text'],tokens=tokens))
            raw.append(dict(name=name,range=[start,end],condition=condition,result=result,canonical_prompt=False))
    write('window-transcripts.json',dict(schema='reel.local-song-asr-window-transcripts.v1',windows=raw,
        recording=descriptor('recording.wav'),runtime=descriptor('runtime.json'),device='cpu',dtype='float32',canonical_prompt=False))
    revised=dict(prior);revised['passes']=prior['passes']+extras
    revised.update(line_report(request['canonical_lyrics'],revised['passes']))
    revised.update(previous_analysis=descriptor('prior-analysis.json'),additional_window_evidence=descriptor('window-transcripts.json'),
        new_recording=False,recognition_revision=True,additional_recognizer='Frozen Whisper, CPU float32, bounded unprompted independent windows')
    write('analysis.json',revised)
    print('WINDOW RECHECK',revised['qualifying_lines'],'/',revised['total_lines'],flush=True)


if __name__=='__main__':main()
