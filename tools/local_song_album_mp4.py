"""Native-job album container export: restrained labels and copied MP3 packets.

This exports an existing compiled album; it does not assemble scenes, replace
song performances or provide a narrative rendering grammar.
"""
import json
import hashlib
import math
from pathlib import Path
import shutil
import subprocess
import textwrap

from local_song_backend import checked, descriptor, offline, read, write


def rgb(value):
    value=value.lstrip('#')
    if len(value)!=6 or any(c not in '0123456789abcdefABCDEF' for c in value):raise ValueError('Invalid print palette')
    return value[4:6]+value[2:4]+value[:2]


def wrapped(text, width=24):
    if not isinstance(text,str) or not text.strip():raise ValueError('Empty bilingual reading line')
    if any(c in text for c in ['{','}','\\','\r','\n']):raise ValueError('Reading line contains layout controls')
    return textwrap.wrap(text,width=width,break_long_words=False,break_on_hyphens=False) or ['']


def reading_labels(plan, check):
    """A fixed bilingual print template over an existing album clock.

    Page holds are editorial reading times, never claimed word entrances.
    """
    if plan['schema']!='reel.album-bilingual-reading.v1' or plan['mode']!='editorial-reading-pages':raise ValueError('Unsupported bilingual reading')
    if [t['song_id'] for t in plan['tracks']]!=[t['song_id'] for t in check['tracks']]:raise ValueError('Bilingual pages belong to a different album')
    seconds=check['duration_seconds'];palette=plan['palette'];ink=rgb(palette['ink']);accent=rgb(palette['accent']);rust=rgb(palette['secondary']);soft=rgb(palette['rule'])
    result='''[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 2
[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
'''
    result+=f'Style: Default,Georgia,64,&H00{ink},&H00{ink},&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n'
    def text(start,end,x,y,value,size=64,color=ink,bold=False,font='Georgia',align=7):
        nonlocal result
        result+=f'Dialogue: 3,{ass_time(start)},{ass_time(end)},Default,,0,0,0,,{{\\an{align}\\pos({x},{y})\\fs{size}\\1c&H{color}&\\b{int(bold)}\\fn{font}}}'+value+'\n'
    def rectangle(x,y,width,height,color):
        nonlocal result
        path=f'm {x} {y} l {x+width} {y} {x+width} {y+height} {x} {y+height}'
        result+=f'Dialogue: 0,0:00:00.00,{ass_time(seconds+.2)},Default,,0,0,0,,{{\\an7\\pos(0,0)\\p1\\bord0\\shad0\\1c&H{color}&}}'+path+'\n'
    # Stepped lithographic frame; ornament never occupies the reading columns.
    for x,y,width,height in [(48,48,1824,3),(48,1029,1824,3),(48,48,3,984),(1869,48,3,984),(64,64,1792,1),(64,1014,1792,1)]:rectangle(x,y,width,height,soft)
    for x,y,sx,sy in [(48,48,1,1),(1872,48,-1,1),(48,1032,1,-1),(1872,1032,-1,-1)]:
        for step in range(3):
            xx=x+sx*(12+step*9);yy=y+sy*(12+step*9)
            rectangle(min(xx,xx+sx*90),yy,90,3,accent);rectangle(xx,min(yy,yy+sy*90),3,90,accent)
    rectangle(118,244,1684,3,rust);rectangle(958,300,2,632,soft)
    # Small fan ornament between the language headings, outside both columns.
    for i in range(13):
        angle=math.pi+i*math.pi/12;x=960+round(42*math.cos(angle));y=300+round(42*math.sin(angle))
        path=f'm 960 300 l {x-1} {y} {x+1} {y}'
        result+=f'Dialogue: 1,0:00:00.00,{ass_time(seconds+.2)},Default,,0,0,0,,{{\\an7\\pos(0,0)\\p1\\bord0\\shad0\\1c&H{rust}&}}'+path+'\n'
    text(0,seconds+.2,118,83,label(check['title'].upper(),70),30,accent,True,'Trebuchet MS')
    text(0,seconds+.2,118,966,'EL CAMINO DE LOS CAIMITOS',25,accent,True,'Trebuchet MS')
    ledger=[]
    for i,(reading,track) in enumerate(zip(plan['tracks'],check['tracks'])):
        start=track['start_frame']/check['sample_rate'];end=check['tracks'][i+1]['start_frame']/check['sample_rate'] if i+1<len(check['tracks']) else seconds+.2
        text(start,end,118,132,label(track['title'],58),52,ink,True)
        text(start,end,1800,87,f'{i+1:02} / {len(check["tracks"]):02}',30,rust,True,'Trebuchet MS',9)
        if reading['mode'] in ['instrumental','wordless-coro']:
            text(start,end,960,500,'Instrumental' if reading['mode']=='instrumental' else 'Coro sin palabras',78,accent,False,'Georgia',5)
            text(start,end,960,615,'Música del libro' if reading['mode']=='instrumental' else 'Wordless chorus',38,ink,False,'Georgia',5)
            ledger.append(dict(song_id=track['song_id'],mode=reading['mode'],pages=0));continue
        text(start,end,118,205,'Andrés Alarcón García',25,ink)
        text(start,end,118,278,'ESPAÑOL',30,accent,True,'Trebuchet MS')
        text(start,end,1010,278,'ENGLISH',30,accent,True,'Trebuchet MS')
        pages=reading['pages'];previous=start
        for n,page in enumerate(pages):
            a,b=page['start_frame']/48000,page['end_frame']/48000
            if a<previous-.001 or b<=a or a<start or b>end+.001 or not page['lines']:raise ValueError('Invalid reading-page clock')
            rows=0
            for pair in page['lines']:
                es,en=wrapped(pair['es']),wrapped(pair['en']);count=max(len(es),len(en))
                if rows+count>7:raise ValueError('Reading page is too crowded for the phone template')
                for line_index,line in enumerate(es):text(a,b,118,340+(rows+line_index)*78,line,64)
                for line_index,line in enumerate(en):text(a,b,1010,340+(rows+line_index)*78,line,64)
                rows+=count
            text(a,b,1800,966,f'{n+1} / {len(pages)}',25,accent,False,'Trebuchet MS',9)
            previous=b
        ledger.append(dict(song_id=track['song_id'],mode=reading['mode'],pages=len(pages),timing='editorial reading holds; not karaoke alignment'))
    return result,ledger


def decoded_identity(ff,path):
    process=subprocess.Popen([str(ff),'-v','error','-nostdin','-i',str(path),'-map','0:a:0','-ar','48000','-ac','2','-f','f32le','-'],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    h=hashlib.sha256();count=0
    while True:
        part=process.stdout.read(1048576)
        if not part:break
        h.update(part);count+=len(part)
    error=process.stderr.read().decode('utf-8',errors='replace');code=process.wait();process.stdout.close();process.stderr.close()
    if code or count%8:raise ValueError('PCM identity decode failed: '+error)
    return dict(frames=count//8,sha256=h.hexdigest())


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
    for i,item in enumerate(q.get('additional_fonts',[])):
        shutil.copyfile(checked(item['path'],item),f'fonts/face-{i}.ttf')
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
    width,height,background=1280,720,'0x23302b';reading_ledger=None
    if q.get('reading_plan'):
        reading=read(checked(q['reading_plan']['path'],q['reading_plan']))
        if reading['source_album_check']['sha256']!=q['album_check']['sha256']:raise ValueError('Reading pages use another audio master')
        header,reading_ledger=reading_labels(reading,check);width,height=1920,1080;background='0x'+reading['palette']['paper'].lstrip('#')
        write('reading-layout-check.json',dict(template='cuban-print-bilingual-v1',canvas=[width,height],body_font_px=64,reading_columns=['Spanish','English'],page_row_limit=7,reading_timing='Editorial reading pages, not word-synced underlay',tracks=reading_ledger))
    Path('labels.ass').write_text(header,encoding='utf-8',newline='\n')
    picture_seconds=math.ceil(seconds*5)/5
    subprocess.run([str(ff),'-v','error','-nostdin','-f','lavfi','-i',f'color=c={background}:s={width}x{height}:r=5:d={picture_seconds}',
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
    pcm=decoded_identity(ff,'master.mp4') if 'mp3_decode' in check else None
    if pcm and (pcm['frames']!=check['mp3_decode']['frames'] or pcm['sha256']!=check['mp3_decode']['sha256']):
        raise ValueError('Decoded MP4 audio differs from the completed album master')
    write('mp4-check.json',dict(schema='reel.local-song-album-mp4-check.v1',request=descriptor('mp4-request.json'),
        source_audio=q['audio'],source_album_check=q['album_check'],mp4=descriptor('master.mp4'),
        title=q['title'],track_count=len(tracks),audio_packet_count=len(source_packets),
        all_audio_packet_payloads_exact=True,audio_reencoded=False,audio_trimmed=False,full_decode=True,
        source_duration_seconds=seconds,container_duration_seconds=float(info['format']['duration']),
        video=dict(codec='h264',width=width,height=height,fps=5),audio_codec='mp3',font=q['font'],decoded_pcm_identity=pcm,
        reading_plan=q.get('reading_plan'),reading_layout=reading_ledger,
        title_labels=descriptor('labels.ass'),new_human_picture_or_album_listening_approval=False,public_release=False))
    print('MP4 VERIFIED',q['title'],seconds,len(tracks),'tracks; every audio packet preserved',flush=True)


if __name__=='__main__':offline();build(read('mp4-request.json'))
