"""Unprompted source-clock word transcripts using an explicitly frozen local ASR.

This estimates lyric entrances; it does not select text or approve a performance.
"""
import subprocess
import numpy as np
from local_song_backend import checked, offline, read, runtime_files, words, write


def main():
    offline();q=read('transcript-request.json');rt=read('runtime.json')
    checked(rt['ffmpeg']['path'],rt['ffmpeg']);runtime_files(rt,'analysis_files')
    import torch
    import transformers.pipelines.automatic_speech_recognition as asrm
    from transformers import pipeline
    torch.set_num_threads(4);torch.manual_seed(0);asrm.is_torchcodec_available=lambda:False
    if q['device']!='cuda' or not torch.cuda.is_available():raise ValueError('Explicit CUDA recognizer unavailable')
    asr=pipeline('automatic-speech-recognition',model=rt['whisper_root'],device=0,torch_dtype=torch.float32,batch_size=1)
    results=[]
    for row in q['recordings']:
        checked(row['audio']['path'],row['audio'])
        raw=subprocess.check_output([rt['ffmpeg']['path'],'-v','error','-nostdin','-i',row['audio']['path'],'-ar','16000','-ac','1','-f','f32le','-'])
        waveform=np.frombuffer(raw,dtype='<f4');passes=[]
        for length in [30,20]:
            result=asr(dict(raw=waveform,sampling_rate=16000),chunk_length_s=length,stride_length_s=4,
                return_timestamps='word',generate_kwargs=dict(language='spanish',task='transcribe',num_beams=3,
                condition_on_prev_tokens=False,max_new_tokens=256,repetition_penalty=1.1))
            tokens=[]
            for chunk in result.get('chunks',[]):tokens.extend(dict(word=w,span=list(chunk['timestamp'])) for w in words(chunk['text']))
            passes.append(dict(name='unprompted-mix-'+str(length),text=result['text'],tokens=tokens,raw=result))
        results.append(dict(id=row['id'],recording=row['audio'],seconds=len(waveform)/16000,passes=passes,canonical_prompt=False))
        print('SOURCE CLOCK TRANSCRIPT',row['id'],[len(p['tokens']) for p in passes],flush=True)

    write('word-transcripts.json',dict(schema='reel.song-word-transcripts.v1',recordings=results,canonical_prompt=False))


if __name__=='__main__':main()
