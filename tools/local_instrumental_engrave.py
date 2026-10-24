"""Engrave measured MusicXML and audit the score-app round trip independently.

This adapter never renders arrangement audio or treats a transport check as
musical approval. Source and re-exported pitched intervals must agree exactly.
"""
from collections import Counter
from fractions import Fraction
from pathlib import Path
import os
import subprocess
import zipfile
import copy
import math
import unicodedata
import xml.etree.ElementTree as ET
from local_song_backend import checked, descriptor, offline, read, write


def duration_pieces(duration,divisions):
    options=[]
    for denominator,name in [(1,'whole'),(2,'half'),(4,'quarter'),(8,'eighth'),(16,'16th'),(32,'32nd'),(64,'64th'),(128,'128th')]:
        for dots in [0,1,2]:
            value=Fraction(divisions*4,denominator)*sum((Fraction(1,2**d) for d in range(dots+1)),Fraction(0))
            if value.denominator==1:options.append((int(value),name,dots,None))
    result=[]
    while duration:
        candidates=[v for v in options if v[0]<=duration]
        if not candidates:
            gcd=math.gcd(duration,divisions);result.append((duration,'quarter',0,(divisions//gcd,duration//gcd)));break
        chosen=max(candidates,key=lambda v:v[0]);result.append(chosen);duration-=chosen[0]
    return result


def prepare_musicxml(source,target,omit_trailing_silent_measures=False,page_format=None):
    """Spell identical intervals as explicit tied durations for score-app import."""
    tree=ET.parse(source);root=tree.getroot();pieces=0
    if page_format is not None:
        if page_format!='a3':raise ValueError('Unsupported explicit score page format')
        defaults=root.find('defaults')
        if defaults is None:
            defaults=ET.Element('defaults');root.insert(next(i for i,e in enumerate(root) if e.tag in ['credit','part-list']),defaults)
        for tag in ['scaling','page-layout']:
            for element in defaults.findall(tag):defaults.remove(element)
        scaling=ET.Element('scaling');ET.SubElement(scaling,'millimeters').text='7';ET.SubElement(scaling,'tenths').text='40';defaults.insert(0,scaling)
        layout=ET.SubElement(defaults,'page-layout');ET.SubElement(layout,'page-height').text='2400';ET.SubElement(layout,'page-width').text=str(297*40/7);margins=ET.SubElement(layout,'page-margins',type='both')
        for tag in ['left-margin','right-margin','top-margin','bottom-margin']:ET.SubElement(margins,tag).text='80'
    for part in root.findall('part'):
        divisions=1
        for measure in part.findall('measure'):
            children=list(measure);output=[];index=0
            while index<len(children):
                child=children[index];index+=1
                if child.tag=='attributes' and child.findtext('divisions') is not None:divisions=int(child.findtext('divisions'))
                if child.tag!='note':output.append(child);continue
                group=[child]
                while index<len(children) and children[index].tag=='note' and children[index].find('chord') is not None:group.append(children[index]);index+=1
                durations={int(n.findtext('duration')) for n in group}
                if len(durations)!=1:raise ValueError('A chord has unequal note lengths')
                segments=duration_pieces(durations.pop(),divisions)
                for si,(duration,kind,dots,modification) in enumerate(segments):
                    for ni,original in enumerate(group):
                        n=copy.deepcopy(original);old_ties={t.attrib['type'] for t in n.findall('tie')};rest=n.find('rest') is not None
                        for element in list(n):
                            if element.tag in ['chord','duration','tie','voice','type','dot','time-modification','staff']:n.remove(element)
                        notation=n.find('notations')
                        if notation is not None:
                            for tied in notation.findall('tied'):notation.remove(tied)
                            if not list(notation):n.remove(notation);notation=None
                        if ni:n.insert(0,ET.Element('chord'))
                        order=next((j+1 for j,e in enumerate(n) if e.tag in ['pitch','unpitched','rest']),len(n));new=[]
                        e=ET.Element('duration');e.text=str(duration);new.append(e)
                        ties=[] if rest else (['stop'] if si or 'stop' in old_ties else [])+(['start'] if si<len(segments)-1 or 'start' in old_ties else [])
                        for tie in ties:new.append(ET.Element('tie',type=tie))
                        voice=int(original.findtext('voice','1'));staff=int(original.findtext('staff','1'));e=ET.Element('voice');e.text=str(voice+(staff-1)*4);new.append(e)
                        e=ET.Element('type');e.text=kind;new.append(e)
                        new.extend(ET.Element('dot') for _ in range(dots))
                        if modification:
                            e=ET.Element('time-modification');ET.SubElement(e,'actual-notes').text=str(modification[0]);ET.SubElement(e,'normal-notes').text=str(modification[1]);ET.SubElement(e,'normal-type').text='quarter';new.append(e)
                        if original.find('staff') is not None:e=ET.Element('staff');e.text=str(staff);new.append(e)
                        for offset,e in enumerate(new):n.insert(order+offset,e)
                        if ties:
                            if notation is None:notation=ET.SubElement(n,'notations')
                            for tie in ties:ET.SubElement(notation,'tied',type=tie)
                        output.append(n);pieces+=1
            measure[:]=output
    if omit_trailing_silent_measures:
        parts=root.findall('part')
        while parts and all(len(p.findall('measure'))>1 and not any(n.find('rest') is None for n in p.findall('measure')[-1].findall('note')) for p in parts):
            for part in parts:part.remove(part.findall('measure')[-1])
    tree.write(target,encoding='utf-8',xml_declaration=True)
    if intervals(source)!=intervals(target):raise ValueError('Explicit duration spelling changed measured intervals')
    return pieces


def intervals(path):
    root=ET.parse(path).getroot();result=[]
    instruments={i.attrib['id']:int(i.findtext('midi-unpitched'))-1 for i in root.findall('./part-list/score-part/midi-instrument') if i.findtext('midi-unpitched') is not None}
    for pi,part in enumerate(root.findall('part')):
        clock=Fraction(0);divisions=1;pending={}
        for measure in part.findall('measure'):
            cursor=Fraction(0);extent=Fraction(0);last=Fraction(0)
            for child in measure:
                if child.tag=='attributes' and child.findtext('divisions') is not None:divisions=int(child.findtext('divisions'))
                if child.tag in ['backup','forward']:
                    cursor+=Fraction(int(child.findtext('duration')),divisions)*(-1 if child.tag=='backup' else 1);extent=max(extent,cursor)
                if child.tag!='note':continue
                if child.find('grace') is not None:raise ValueError('An unmeasured grace note entered the engraved projection')
                duration=Fraction(int(child.findtext('duration')),divisions);onset=last if child.find('chord') is not None else cursor
                if child.find('chord') is None:last=onset;cursor+=duration
                extent=max(extent,onset+duration)
                if child.find('rest') is not None:continue
                p=child.find('pitch')
                if p is not None:pitch=12*(int(p.findtext('octave'))+1)+{'C':0,'D':2,'E':4,'F':5,'G':7,'A':9,'B':11}[p.findtext('step')]+int(p.findtext('alter','0'))
                else:
                    instrument=child.find('instrument');pitch=instruments.get(instrument.attrib['id']) if instrument is not None else None
                    if pitch is None:raise ValueError('Percussion MIDI identity missing in the score-app round trip')
                key=(pitch,child.findtext('voice','1'),child.findtext('staff','1'));start=clock+onset;end=start+duration;ties={t.attrib['type'] for t in child.findall('tie')}
                if 'stop' in ties:
                    prior=pending.pop(key,None)
                    if prior is None or prior[1]!=start:raise ValueError('Broken tie in measured MusicXML')
                    start=prior[0]
                if 'start' in ties:
                    if key in pending:raise ValueError('Overlapping tie identity')
                    pending[key]=(start,end)
                else:result.append((pi,pitch,start,end))
            clock+=extent
        if pending:raise ValueError('Unfinished tied notes in engraved score')
    return Counter(result)


def text_bounds(blocks,width,height):
    """Inspect text spans; a music glyph's metrics must not engulf chord text."""
    outside=[];font_overhangs=[]
    for block in blocks:
        for line in block.get('lines',[]):
            for span in line['spans']:
                box=span['bbox']
                if box[0]<-1 or box[1]<-1 or box[2]>width+1 or box[3]>height+1:
                    destination=outside if any(not ch.isspace() and unicodedata.category(ch)!='Co' for ch in span['text']) else font_overhangs
                    destination.append(list(box))
    return outside,font_overhangs


def main():
    offline();q=read('engraving-request.json')
    if q['schema']!='reel.instrumental-engraving-request.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Explicit private engraving request required')
    checked('score.musicxml',q['musicxml']);ms=checked(q['musescore']['path'],q['musescore']);env=os.environ.copy();env['QT_QPA_PLATFORM']='windows:fontengine=freetype';env['QT_LOGGING_RULES']='*.debug=false'
    for item in q['runtime_files']:checked(item['path'],item)
    fragments=prepare_musicxml('score.musicxml','engraving-input.musicxml',q.get('omit_trailing_silent_measures',False),q.get('page_format'))
    for output in ['engraved.pdf','roundtrip.musicxml']:
        proc=subprocess.run([str(ms),'-f','-o',output,'engraving-input.musicxml'],env=env,capture_output=True,text=True,encoding='utf-8',errors='replace',timeout=600)
        Path(output+'.log').write_text(proc.stdout+'\n'+proc.stderr,encoding='utf-8')
        if proc.returncode or not Path(output).is_file():raise ValueError('Score application did not produce '+output)
    expected=intervals('score.musicxml');actual=intervals('roundtrip.musicxml')
    audit=dict(source_notes=sum(expected.values()),engraved_notes=sum(actual.values()),explicit_fragments=fragments,engraving_input=descriptor('engraving-input.musicxml'),missing=[list(map(str,x))+[n] for x,n in (expected-actual).items()],extra=[list(map(str,x))+[n] for x,n in (actual-expected).items()],exact_intervals=expected==actual)
    write('roundtrip-check.json',audit)
    if expected!=actual:raise ValueError('Score application changed measured pitches or nominal musical intervals')
    import fitz
    document=fitz.open('engraved.pdf');pages=[]
    for i,page in enumerate(document):
        pix=page.get_pixmap(matrix=fitz.Matrix(1.25,1.25),alpha=False);filename='qa-page-'+str(i+1)+'.png';pix.save(filename);data=pix.samples;ink=sum(v<240 for v in data)/len(data)
        outside,font_overhangs=text_bounds(page.get_text('dict')['blocks'],page.rect.width,page.rect.height)
        stride=pix.stride;channels=pix.n;edge_ink=0
        for y in range(pix.height):
            xs=range(pix.width) if y<3 or y>=pix.height-3 else [0,1,2,pix.width-3,pix.width-2,pix.width-1]
            edge_ink+=sum(min(data[y*stride+x*channels:y*stride+x*channels+3])<200 for x in xs)
        pages.append(dict(page=i+1,width=page.rect.width,height=page.rect.height,ink_fraction=ink,text_outside_page=outside,musical_font_metric_overhangs=font_overhangs,raster_edge_ink_pixels=edge_ink,render=descriptor(filename)))
        if ink<.0001 or outside or edge_ink:raise ValueError('Blank page, clipped raster ink or text outside PDF page')
    with zipfile.ZipFile('rendered-qa.zip','w',compression=zipfile.ZIP_DEFLATED) as archive:
        for i in range(len(pages)):archive.write('qa-page-'+str(i+1)+'.png')
    write('print-check.json',dict(pages=pages,page_count=len(pages),pdf=descriptor('engraved.pdf'),roundtrip=audit,method='Producer-applied rendered pixel and page geometry checks; images not displayed',human_visual_approval=False,musical_approval=False))
    print('ENGRAVING AND EXACT NOTE ROUND TRIP VERIFIED',sum(expected.values()),'notes',len(pages),'pages',flush=True)


if __name__=='__main__':main()
