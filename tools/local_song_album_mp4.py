"""Native-job album container export: restrained labels and copied MP3 packets.

This exports an existing compiled album; it does not assemble scenes, replace
song performances or provide a narrative rendering grammar.
"""
import json
import math
from pathlib import Path
import shutil
import subprocess
import textwrap

from local_song_backend import checked, descriptor, offline, read, write


def ass_time(seconds):
    ticks=round(seconds*100)
    return f'{ticks//360000}:{ticks//6000%60:02}:{ticks//100%60:02}.{ticks%100:02}'


def label(text, width=40):
    return r'\N'.join(textwrap.wrap(str(text).replace('\\','/').replace('{','(').replace('}',')'),width))


def probe(ffprobe, path, packets=False):
    args=[str(ffprobe),'-v','error']
    if packets:
        args+=['-select_streams','a:0','-show_packets','-show_data_hash','sha256',
               '-show_entries','packet=data_hash,size,pts_time,dts_time,duration_time,side_data_list']
    else:args+=['-show_streams','-show_format','-show_chapters']
    return json.loads(subprocess.check_output(args+['-of','json',str(path)]))


def build(q):
    if q['schema']!='reel.local-song-album-mp4.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):
        raise ValueError('Explicit private compiled-album export required')
    ff=checked(q['ffmpeg']['path'],q['ffmpeg']);fp=checked(q['ffprobe']['path'],q['ffprobe'])
    audio=checked('album.mp3',q['audio']);checked('chapters.ffmetadata',q['chapters']);checked('album-check.json',q['album_check'])
    check=read('album-check.json');tracks=check['tracks'];seconds=check['duration_seconds']
    if not 1<=len(tracks)<=128 or not 1<=seconds<=14400 or q['title']!=check['title']:
        raise ValueError('Compiled album identity or extent differs')
    font=checked(q['font']['path'],q['font']);Path('fonts').mkdir();shutil.copyfile(font,'fonts/album.ttf')
    header='''[Script Info]
ScriptType: v4.00+
PlayResX: 1280
PlayResY: 720
WrapStyle: 0
[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Segoe UI,48,&H00E7F2F6,&H00E7F2F6,&H00302B23,&H00302B23,0,0,0,0,100,100,0,0,1,0,0,5,70,70,40,1
[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
'''
    header+=f'Dialogue: 0,0:00:00.00,{ass_time(seconds+.2)},Default,,0,0,0,,{{\\pos(640,140)\\fs34}}'+label(q['title'],48)+'\n'
    for i,t in enumerate(tracks):
        start=t['start_frame']/check['sample_rate'];end=tracks[i+1]['start_frame']/check['sample_rate'] if i+1<len(tracks) else seconds+.2
        header+=f'Dialogue: 1,{ass_time(start)},{ass_time(end)},Default,,0,0,0,,{{\\pos(640,360)\\fs48}}'+label(t['title'])+'\n'
        header+=f'Dialogue: 1,{ass_time(start)},{ass_time(end)},Default,,0,0,0,,{{\\pos(640,585)\\fs25}}'+f'{i+1:02} / {len(tracks):02}'+'\n'
    Path('labels.ass').write_text(header,encoding='utf-8',newline='\n')
    picture_seconds=math.ceil(seconds*5)/5
    subprocess.run([str(ff),'-v','error','-nostdin','-f','lavfi','-i',f'color=c=0x23302b:s=1280x720:r=5:d={picture_seconds}',
        '-i',str(audio),'-f','ffmetadata','-i','chapters.ffmetadata','-map','0:v:0','-map','1:a:0',
        '-vf','ass=labels.ass:fontsdir=fonts','-map_metadata','2','-map_chapters','2',
        '-c:v','libx264','-preset','medium','-crf','20','-pix_fmt','yuv420p','-c:a','copy',
        '-movflags','+faststart','-video_track_timescale','48000','master.mp4'],check=True)
    source=probe(fp,audio,True);output=probe(fp,'master.mp4',True)
    write('source-audio-packets.json',source);write('mp4-audio-packets.json',output)
    source_packets=[(p['size'],p['data_hash']) for p in source['packets']]
    output_packets=[(p['size'],p['data_hash']) for p in output['packets']]
    if not source_packets or source_packets!=output_packets:
        raise ValueError('MP4 did not preserve every encoded album audio packet in order')
    info=probe(fp,'master.mp4');write('mp4-probe.json',info)
    video=[s for s in info['streams'] if s['codec_type']=='video'];sound=[s for s in info['streams'] if s['codec_type']=='audio']
    if len(video)!=1 or video[0]['codec_name']!='h264' or len(sound)!=1 or sound[0]['codec_name']!='mp3' or len(info['chapters'])!=len(tracks):
        raise ValueError('MP4 codec or chapter contract differs')
    if not seconds-.05<=float(info['format']['duration'])<=seconds+.3:
        raise ValueError('MP4 changed full album duration')
    subprocess.run([str(ff),'-v','error','-nostdin','-i','master.mp4','-f','null','-'],capture_output=True,check=True)
    write('mp4-check.json',dict(schema='reel.local-song-album-mp4-check.v1',request=descriptor('mp4-request.json'),
        source_audio=q['audio'],source_album_check=q['album_check'],mp4=descriptor('master.mp4'),
        title=q['title'],track_count=len(tracks),audio_packet_count=len(source_packets),
        all_audio_packet_payloads_exact=True,audio_reencoded=False,audio_trimmed=False,full_decode=True,
        source_duration_seconds=seconds,container_duration_seconds=float(info['format']['duration']),
        video=dict(codec='h264',width=1280,height=720,fps=5),audio_codec='mp3',font=q['font'],
        title_labels=descriptor('labels.ass'),new_human_picture_or_album_listening_approval=False,public_release=False))
    print('MP4 VERIFIED',q['title'],seconds,len(tracks),'tracks; every audio packet preserved',flush=True)


if __name__=='__main__':offline();build(read('mp4-request.json'))
