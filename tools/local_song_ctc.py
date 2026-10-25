"""Offline independent Spanish CTC check of bounded retained vocal windows.

Invoke through a declared Rust job. No downloads, source prompts or generation.
"""
from itertools import groupby
import argparse
import os
from pathlib import Path
import subprocess
import time

from local_song_backend import checked, descriptor, line_report, offline, read, words, write


def uncertain_windows(report):
    missing=[i for i,row in enumerate(report['lines']) if not row['passes_gate']]
    if report['passed'] or not 1<=len(missing)<=3:return []
    duration=report['technical']['seconds'];windows=[]
    for i in missing:
        before=next((row['recognized_span'] for row in reversed(report['lines'][:i]) if row.get('recognized_span')),None)
        after=next((row['recognized_span'] for row in report['lines'][i+1:] if row.get('recognized_span')),None)
        spans=[span for span in [before,after] if span and all(t is not None for t in span)]
        if not spans:continue
        start=max(0,min(span[0] for span in spans)-4) if before else 0
        end=min(duration,max(span[1] for span in spans)+4) if after else duration
        if 0<end-start<=60:windows.append([start,end])
    merged=[]
    for start,end in sorted(windows):
        if merged and start<=merged[-1][1]:merged[-1][1]=max(merged[-1][1],end)
        else:merged.append([start,end])
    result=[];total=0
    for start,end in merged:
        while start<end and len(result)<4 and total<180:
            stop=min(end,start+60,start+180-total)
            result.append([start,stop]);total+=stop-start;start=stop
    return result


def prepare_automatic():
    report=read('analysis.json');ranges=uncertain_windows(report)
    if not ranges:
        os.link('analysis.json','prior-analysis.json')
        skipped=dict(schema='reel.local-song-ctc-skip.v1',reason='Already qualified, more than three gaps, or no bounded neighboring time window',
            prior_analysis=descriptor('prior-analysis.json'),component=descriptor('component.json'),canonical_prompt=False)
        write('ctc.json',skipped);write('ctc-transcript.json',skipped)
        print('Independent CTC not needed or no bounded near-pass window',flush=True)
        return False
    os.replace('analysis.json','prior-analysis.json')
    write('ctc.json',dict(schema='reel.local-song-ctc-check.v1',prior_analysis=descriptor('prior-analysis.json'),
        vocal_estimate=descriptor('vocals.wav'),recording=descriptor('recording.wav'),ranges=ranges,
        window_selection='Bounded neighboring recognized spans for up to three failed source lines; no decoder lyric prompt'))
    return True


def decode_ids(ids, labels, frame_seconds, offset):
    tokens=[];letters=[];start=end=None
    def flush():
        nonlocal letters,start,end
        for word in words(''.join(letters)):
            tokens.append(dict(word=word,span=[start,end]))
        letters=[];start=end=None
    for label, group in groupby(enumerate(ids),key=lambda item:item[1]):
        frames=[item[0] for item in group]
        if label==0:continue
        char=labels[label]
        if char=='|':flush();continue
        if start is None:start=offset+frames[0]*frame_seconds
        end=offset+(frames[-1]+1)*frame_seconds
        letters.append(char)
    flush()
    return tokens


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--automatic',action='store_true');args=parser.parse_args()
    if args.automatic and not prepare_automatic():return
    offline();started=time.time()
    cfg=read('ctc.json');request=read('request.json');prior=read('prior-analysis.json');runtime=read('runtime.json')
    if cfg['schema']!='reel.local-song-ctc-check.v1':raise ValueError('Unsupported CTC check')
    checked('source.txt',request['source']);checked('request.json',prior['request'])
    checked('prior-analysis.json',cfg['prior_analysis']);checked('vocals.wav',cfg['vocal_estimate'])
    if prior['recording']!=cfg['recording']:raise ValueError('Original recording binding differs')
    checked(runtime['python']['path'],runtime['python']);checked(runtime['ffmpeg']['path'],runtime['ffmpeg'])
    component=read('component.json')
    if component['schema']!='reel.installed-spanish-ctc.v1':raise ValueError('Unsupported model component')
    model_path=checked(component['model']['path'],component['model'])
    ranges=cfg['ranges'];duration=prior['technical']['seconds']
    if not 1<=len(ranges)<=4 or any(not 0<=a<b<=duration or b-a>60 for a,b in ranges) or sum(b-a for a,b in ranges)>180:
        raise ValueError('CTC window budget exceeded')
    if any(ranges[i][1]>ranges[i+1][0] for i in range(len(ranges)-1)):raise ValueError('Overlapping CTC windows')
    import numpy as np
    import torch
    import torchaudio
    if torchaudio.__version__!=component['torchaudio_version']:raise ValueError('Installed torchaudio revision differs')
    torch.set_num_threads(2);torch.manual_seed(0)
    def no_download(*args,**kwargs):raise RuntimeError('Offline CTC: model download forbidden')
    torch.hub.download_url_to_file=no_download
    bundle=torchaudio.pipelines.VOXPOPULI_ASR_BASE_10K_ES
    if Path(bundle._path).name!=model_path.name:raise ValueError('Model name differs from installed bundle')
    model=bundle.get_model(dl_kwargs=dict(model_dir=str(model_path.parent),file_name=model_path.name,map_location='cpu',weights_only=True)).eval()
    labels=bundle.get_labels();tokens=[];raw=[];extras=[]
    for start,end in ranges:
        print('Independent CPU CTC window',start,end,flush=True)
        data=subprocess.check_output([runtime['ffmpeg']['path'],'-v','error','-ss',str(start),'-t',str(end-start),
            '-i','vocals.wav','-ac','1','-ar','16000','-f','f32le','-'])
        waveform=np.frombuffer(data,dtype=np.float32).copy()
        if abs(len(waveform)/16000-(end-start))>.05 or not np.isfinite(waveform).all():raise ValueError('Vocal window decode failed')
        with torch.inference_mode():emission,_=model(torch.from_numpy(waveform)[None])
        ids=emission[0].argmax(-1).tolist();step=len(waveform)/16000/len(ids)
        found=decode_ids(ids,labels,step,start);tokens.extend(found)
        raw.append(dict(range=[start,end],frame_seconds=step,ids=ids,tokens=found))
        extras.append(dict(name=f'independent-spanish-ctc-cpu-window-{len(raw)}',text=' '.join(t['word'] for t in found),tokens=found))
    write('ctc-transcript.json',dict(schema='reel.local-song-ctc-transcript.v1',component=descriptor('component.json'),
        recording=cfg['recording'],vocal_estimate=cfg['vocal_estimate'],labels=labels,windows=raw,
        dtype='float32',device='cpu',canonical_prompt=False,elapsed_seconds=time.time()-started))
    revised=dict(prior);revised['passes']=prior['passes']+extras
    revised.update(line_report(request['canonical_lyrics'],revised['passes']))
    revised.update(previous_analysis=descriptor('prior-analysis.json'),additional_evidence=descriptor('ctc-transcript.json'),
        additional_recognizer='Installed Spanish CTC, CPU float32, greedy character decoding, no language model or source prompt',
        new_recording=False,recognition_revision=True)
    write('analysis.json',revised)
    print('CTC RECHECK',revised['qualifying_lines'],'/',revised['total_lines'],'prior',prior['qualifying_lines'],flush=True)


if __name__=='__main__':main()
