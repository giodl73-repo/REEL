"""Bounded chaptered listening copy: unchanged video packets, one AAC encode.

Run through the native asset job contract with exact selected source bindings.
Audio joins use decoded sample counts, never rounded video-container durations.
"""
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess

from local_song_backend import checked, descriptor, offline, read, write
from local_song_album_master import decoded, metadata_text


def choose_bitrate(video_bytes, seconds, maximum_bytes, requested=128000):
    if not seconds > 0 or not 0 < video_bytes < maximum_bytes or requested not in [96000,112000,128000,160000,192000]:
        raise ValueError('Invalid compact collection budget')
    reserve=min(3000000,maximum_bytes*.03)
    available=math.floor((maximum_bytes-video_bytes-reserve)*8/seconds/8000)*8000
    bitrate=min(requested,available)
    if bitrate<96000:raise ValueError('Budget cannot retain video and the minimum listening bitrate')
    return bitrate


def probe(exe,path,selector,packets=True):
    return json.loads(subprocess.check_output([str(exe),'-v','error','-select_streams',selector,
        '-show_streams',*(['-show_packets'] if packets else []),'-show_data_hash','sha256','-show_entries',
        'stream=codec_name,width,height,r_frame_rate,extradata_hash,sample_rate,channels,start_time,bit_rate,duration,duration_ts,time_base:packet=pts_time,dts_time,duration_time,size,data_hash',
        '-of','json',str(path)],encoding='utf-8'))


def build(q):
    if q['schema']!='reel.compact-chaptered-collection.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):
        raise ValueError('Explicit private compact collection request required')
    ff=checked(q['ffmpeg']['path'],q['ffmpeg']);fp=checked(q['ffprobe']['path'],q['ffprobe'])
    chapters=q['chapters']
    if not 1<=len(chapters)<=8 or len({c['id'] for c in chapters})!=len(chapters):raise ValueError('Invalid chapter order')
    sources=[];cursor=0;concat='ffconcat version 1.0\n';metadata=';FFMETADATA1\ntitle='+metadata_text(q['title'])+'\n';joint=hashlib.sha256();video_bytes=0;expected_packets=[];codec=None
    for row in chapters:
        for role in ['video','audio']:
            ref=row[role]
            if not re.fullmatch(r'[A-Za-z0-9_.-]+',ref['path']):raise ValueError('Job-local source basename required')
            checked(ref['path'],ref)
        audio_format=probe(fp,row['audio']['path'],'a:0',packets=False)['streams'][0]
        if audio_format['sample_rate']!='48000' or audio_format['channels']!=2:raise ValueError('Selected stereo 48-kHz master required; no source resampling')
        pcm=decoded(ff,row['audio']['path'],'f32le',8,accumulator=joint)
        if pcm['sha256']!=row['decoded_audio']['sha256'] or pcm['frames']!=row['decoded_audio']['frames']:raise ValueError('Source audio differs from selected master')
        info=probe(fp,row['video']['path'],'v:0');stream=info['streams'][0]
        identity={k:stream[k] for k in ['codec_name','width','height','r_frame_rate','extradata_hash']}
        if identity['codec_name']!='h264' or (codec is not None and codec!=identity):raise ValueError('Video streams cannot be copied together')
        codec=identity;seconds=pcm['frames']/48000
        for packet in info['packets']:
            video_bytes+=int(packet['size']);expected_packets.append((int(packet['size']),packet['data_hash'],float(packet['pts_time'])+cursor/48000))
        # Isolate the copied picture stream first: MP3 priming packets in the
        # original container otherwise shift the concat demuxer's video clock.
        picture=row['video']['path']+'.picture.mp4'
        subprocess.run([str(ff),'-v','error','-nostdin','-i',row['video']['path'],'-map','0:v:0','-c:v','copy',
            '-map_metadata','-1','-map_chapters','-1','-video_track_timescale','48000',picture],check=True)
        concat+=f"file '{picture}'\nduration {seconds:.9f}\n"
        metadata+=f"[CHAPTER]\nTIMEBASE=1/48000\nSTART={cursor}\nEND={cursor+pcm['frames']}\ntitle={metadata_text(row['title'])}\n"
        sources.append(dict(id=row['id'],title=row['title'],start_frame=cursor,frames=pcm['frames'],video=row['video'],audio=row['audio'],audio_format=audio_format,decoded_audio=pcm,video_probe=info));cursor+=pcm['frames']
    seconds=cursor/48000;bitrate=choose_bitrate(video_bytes,seconds,q['maximum_bytes'],q['audio_bitrate'])
    Path('videos.ffconcat').write_text(concat,encoding='utf-8',newline='\n');Path('chapters.ffmetadata').write_text(metadata,encoding='utf-8',newline='\n');write('source-probes.json',dict(chapters=sources,expected_video_packet_count=len(expected_packets)))
    args=[str(ff),'-v','error','-nostdin']
    for row in chapters:args+=['-i',row['audio']['path']]
    graph=''.join(f'[{i}:a:0]' for i in range(len(chapters)))+f'concat=n={len(chapters)}:v=0:a=1,asplit=2[encode][proof]'
    subprocess.run(args+['-filter_complex',graph,'-map','[encode]','-c:a','aac','-b:a',str(bitrate),'-ar','48000','-ac','2','audio.m4a',
        '-map','[proof]','-c:a','pcm_f32le','-f','hash','-hash','sha256','source-pcm.sha256'],check=True)
    actual=Path('source-pcm.sha256').read_text().strip().split('=')[-1]
    if actual!=joint.hexdigest():raise ValueError('Audio join changed selected source PCM before AAC encoding')
    subprocess.run([str(ff),'-v','error','-nostdin','-f','concat','-safe','0','-auto_convert','0','-i','videos.ffconcat','-i','audio.m4a','-f','ffmetadata','-i','chapters.ffmetadata',
        '-map','0:v:0','-map','1:a:0','-map_metadata','2','-map_chapters','2','-c','copy','-movflags','+faststart','-video_track_timescale','48000','master.mp4'],check=True)
    if Path('master.mp4').stat().st_size>=q['maximum_bytes']:raise ValueError('Compact MP4 exceeds its strict byte ceiling')
    output=probe(fp,'master.mp4','v:0');write('video-packets.json',output);packets=output['packets']
    if len(packets)!=len(expected_packets):raise ValueError('Video frame packet count changed')
    for index,(actual,(size,digest,pts)) in enumerate(zip(packets,expected_packets)):
        if int(actual['size'])!=size or actual['data_hash']!=digest or abs(float(actual['pts_time'])-pts)>.00005:
            raise ValueError(f'Video packet {index} changed: size {actual["size"]}/{size}, payload_equal={actual["data_hash"]==digest}, pts_error={float(actual["pts_time"])-pts}')
    info=json.loads(subprocess.check_output([str(fp),'-v','error','-show_streams','-show_format','-show_chapters','-of','json','master.mp4'],encoding='utf-8'));write('movie-probe.json',info)
    actual_chapters=info['chapters']
    if len(actual_chapters)!=len(chapters):raise ValueError('Wrong chapter count')
    for chapter,source in zip(actual_chapters,sources):
        if chapter['tags']['title']!=source['title'] or abs(float(chapter['start_time'])-source['start_frame']/48000)>.001:raise ValueError('Chapter label or entrance changed')
    audio=[s for s in info['streams'] if s['codec_type']=='audio'];assert len(audio)==1
    if audio[0]['codec_name']!='aac' or audio[0]['channels']!=2 or audio[0]['sample_rate']!='48000' or abs(float(audio[0]['start_time']))>.0001 or abs(float(audio[0]['duration'])-seconds)>.025:raise ValueError('AAC entrance, extent or stereo format changed')
    pcm=decoded(ff,'master.mp4','f32le',8)
    if not cursor<=pcm['frames']<=cursor+1024:raise ValueError('Unexpected audio loss or padding')
    subprocess.run([str(ff),'-v','error','-nostdin','-i','master.mp4','-f','null','-'],check=True)
    write('collection-check.json',dict(schema='reel.compact-chaptered-collection-check.v1',request=descriptor('collection-request.json'),movie=descriptor('master.mp4'),maximum_bytes=q['maximum_bytes'],seconds=seconds,chapters=[{k:s[k] for k in ['id','title','start_frame','frames']} for s in sources],
        video=codec,video_reencoded=False,all_video_packet_payloads_and_pts_exact=True,video_packet_count=len(packets),source_audio_pcm_join_sha256=joint.hexdigest(),source_audio_frames=cursor,audio_reencoded=True,audio_codec='aac',audio_bitrate=bitrate,audio_gain_or_tempo_changed=False,decoded_audio=pcm,full_decode_verified=True,human_listening_review=False,public_release=False))
    print('COMPACT COLLECTION VERIFIED',Path('master.mp4').stat().st_size,'bytes;',len(chapters),'chapters;',bitrate,'AAC; video packets and clocks unchanged',flush=True)


if __name__=='__main__':offline();build(read('collection-request.json'))
