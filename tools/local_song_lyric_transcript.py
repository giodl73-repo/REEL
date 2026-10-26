"""Unprompted source-clock word transcripts using an explicitly frozen local ASR.

This estimates lyric entrances; it does not select text or approve a performance.
"""
import subprocess
import numpy as np
from local_song_backend import checked, offline, read, runtime_files, words, write


def activity_window(waveform, sample_rate=16000, trim=False):
    if type(trim) is not bool:
        raise ValueError('Silence trimming must be explicitly boolean')
    if not trim or not len(waveform):return 0,len(waveform)
    hop=sample_rate//2
    energy=np.array([np.sqrt(np.mean(waveform[i:i+hop]**2)) for i in range(0,len(waveform),hop)])
    active=np.flatnonzero(energy>max(.0001,float(energy.max())*.04))
    if not len(active):return 0,len(waveform)
    return max(0,int(active[0]*hop)-round(1.5*sample_rate)),min(len(waveform),int((active[-1]+1)*hop)+round(1.5*sample_rate))


def shifted_timestamp(pair, offset):
    return [value+offset if value is not None else None for value in pair]


def main():
    offline();q=read('transcript-request.json');rt=read('runtime.json')
    checked(rt['ffmpeg']['path'],rt['ffmpeg']);runtime_files(rt,'analysis_files')
    import torch
    import transformers.pipelines.automatic_speech_recognition as asrm
    from transformers import pipeline
    torch.set_num_threads(4);torch.manual_seed(0);asrm.is_torchcodec_available=lambda:False
    if q['device']!='cuda' or not torch.cuda.is_available():raise ValueError('Explicit CUDA recognizer unavailable')
    dtype=q.get('recognition_dtype','float32')
    if dtype not in ['float32','float16']:raise ValueError('Unsupported explicit recognition precision')
    asr=pipeline('automatic-speech-recognition',model=rt['whisper_root'],device=0,torch_dtype=getattr(torch,dtype),batch_size=1)
    results=[]
    for row in q['recordings']:
        checked(row['audio']['path'],row['audio'])
        raw=subprocess.check_output([rt['ffmpeg']['path'],'-v','error','-nostdin','-i',row['audio']['path'],'-ar','16000','-ac','1','-f','f32le','-'])
        waveform=np.frombuffer(raw,dtype='<f4');passes=[]
        start,end=activity_window(waveform,trim=row.get('trim_silence',False));offset=start/16000
        for length in [30,20]:
            result=asr(dict(raw=waveform[start:end],sampling_rate=16000),chunk_length_s=length,stride_length_s=4,
                return_timestamps='word',generate_kwargs=dict(language='spanish',task='transcribe',num_beams=3,
                condition_on_prev_tokens=False,max_new_tokens=256,repetition_penalty=1.1))
            tokens=[]
            for chunk in result.get('chunks',[]):
                chunk['recognition_timestamp']=list(chunk['timestamp']);chunk['timestamp']=shifted_timestamp(chunk['timestamp'],offset)
                tokens.extend(dict(word=w,span=list(chunk['timestamp'])) for w in words(chunk['text']))
            passes.append(dict(name='unprompted-'+row.get('audio_role','mix')+'-'+str(length),text=result['text'],tokens=tokens,raw=result,source_clock_offset_seconds=offset,recognition_dtype=dtype))
        results.append(dict(id=row['id'],recording=row['audio'],seconds=len(waveform)/16000,passes=passes,canonical_prompt=False,recognized_interval_seconds=[start/16000,end/16000],source_audio_modified=False))
        print('SOURCE CLOCK TRANSCRIPT',row['id'],[len(p['tokens']) for p in passes],flush=True)

    write('word-transcripts.json',dict(schema='reel.song-word-transcripts.v1',recordings=results,canonical_prompt=False))


if __name__=='__main__':main()
