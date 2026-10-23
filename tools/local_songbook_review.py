"""Package existing native notation exports for private print/listening review.

This is a presentation adapter, never a music transcription or notation renderer.
Native SVG geometry and editable scores are preserved without note changes.
"""
import argparse
import copy
import hashlib
import html
import io
import json
import math
from pathlib import Path
import xml.etree.ElementTree as ET


def checked(row):
    path = Path(row['path'])
    if path.stat().st_size != row['bytes'] or hashlib.sha256(path.read_bytes()).hexdigest() != row['sha256']:
        raise ValueError('Review input differs from its frozen hash: ' + str(path))
    return path


def system_pages(root, per_page=4):
    """Use the existing native SVG system grid; reject another exporter layout."""
    ns = {'s': 'http://www.w3.org/2000/svg'}
    starts = sorted({float(e.attrib['y1']) for e in root.findall('s:line', ns)
                     if e.attrib.get('class') == 'staff' and e.attrib.get('x1') == '80'
                     and e.attrib.get('x2') == '1060'})
    systems = starts[::5]
    if not systems or starts != [y + d for y in systems for d in (0, 10, 20, 30, 40)]:
        raise ValueError('Unsupported native SVG staff grid')
    if any(abs(y - (80 + 150*i)) > .01 for i, y in enumerate(systems)):
        raise ValueError('Unsupported native SVG system spacing')
    return [(40 + 150*i, 40 + 150*min(i+per_page, len(systems)))
            for i in range(0, len(systems), per_page)]


def produce(request, output):
    from reportlab.pdfgen import canvas
    from reportlab.pdfbase import pdfmetrics
    from reportlab.pdfbase.ttfonts import TTFont
    from reportlab.graphics import renderPDF
    from svglib.svglib import svg2rlg
    import pymupdf
    output.mkdir(parents=True, exist_ok=True)
    fonts = request['fonts']
    for name, row in fonts.items():
        pdfmetrics.registerFont(TTFont(name, str(checked(row))))
    pdf = output / 'Full Golden Songbook - Working Edition.pdf'
    c = canvas.Canvas(str(pdf), pagesize=(612, 792), invariant=1)
    c.setTitle(request['title'])
    c.setAuthor(request['author'])
    pages = []

    def line(text, x, y, size=11, color='#23352f'):
        c.setFillColor(color); c.setFont('BookText', size); c.drawString(x, y, text)

    def wrap(text, x, y, width, size=11, leading=16):
        words=text.split(); current=''
        for word in words:
            trial=(current+' '+word).strip()
            if current and pdfmetrics.stringWidth(trial,'BookText',size)>width:
                line(current,x,y,size);y-=leading;current=word
            else:current=trial
        if current:line(current,x,y,size);y-=leading
        return y

    def footer(width, title):
        c.setStrokeColorRGB(.75,.8,.77);c.line(36,39,width-36,39)
        line('Private working edition | '+title,36,25,8)
        c.setFont('BookText',8);c.drawRightString(width-36,25,str(len(pages)+1))

    line('SONGBOOK',48,720,12)
    line('Four full Goldens',48,674,31)
    line('Poems by '+request['author'],48,640,14)
    y=wrap('Complete Spanish poems, original full recordings and editable notation studies.',48,595,500,13,20)
    y=wrap('Producer-applied working edition. Musical review pending. The recordings are selected Goldens; the proposed scores are not approved final engravings.',48,y-22,500,12,19)
    for entry in request['entries']:
        y=wrap(entry['title']+'  |  '+entry['edition']+'  |  '+entry['duration'],48,y-24,500,12,18)
    wrap('Notation pages reproduce the native REEL SVG geometry. Missing recovered notes remain gaps. Pulse, meter, pitches, articulation and lyric placement still need musical review. The listening page includes passage buttons and the exact full recordings.',48,235,500,11,17)
    footer(612,request['date']);pages.append(dict(kind='cover'));c.showPage()
    sections=[]
    for index,entry in enumerate(request['entries']):
        title=entry['title'];evidence=json.loads(checked(entry['evidence']).read_text(encoding='utf-8'))
        canonical=checked(entry['canonical']).read_text(encoding='utf-8')
        all_lines=[s.strip() for s in canonical.splitlines() if s.strip()]
        omitted=evidence['owner_accepted_omissions'];timed={p['line'] for p in evidence['phrases']}
        c.setPageSize((612,792));y=wrap(title,48,728,516,22,28)
        y=wrap(entry['edition']+' | '+entry['duration']+' | Full selected recording',48,y-8,516,10,15)
        y=wrap(entry['status'],48,y-7,516,10,15)
        y-=13
        for number,text in enumerate(all_lines,1):
            # Fixed numbered canonical text; do not substitute transcription spelling.
            y=wrap(str(number)+'. '+text,48,y,516,12,15)
        y-=12
        if omitted:y=wrap('Accepted performance omissions: '+', '.join(map(str,omitted))+'. The complete lines above remain available for a future rendition.',48,y,516,10,14)
        y=wrap(f'Timing proposals: {len(timed)}/{len(all_lines)-len(omitted)} performed lines. These do not prove independent word presence.',48,y,516,10,14)
        if y<48:raise ValueError('Canonical poem page overflows')
        footer(612,'Complete canonical poem');pages.append(dict(kind='poem',song=title));c.showPage()
        raw=checked(entry['svg']).read_text(encoding='utf-8');root=ET.fromstring(raw)
        bands=system_pages(root)
        # Font substitution only: preserve all native musical geometry and text.
        for e in root.iter():
            if e.tag.endswith('text'):
                e.set('font-family','BookSymbol' if e.text=='\U0001d11e' else 'BookText')
        for e in root:
            if e.tag.endswith('style'):e.text=e.text.replace('font-family:serif;','')
        drawing=svg2rlg(io.BytesIO(ET.tostring(root)))
        scale=720/1100
        for start,end in bands:
            c.setPageSize((792,612));y=wrap(title,36,578,720,17,22)
            line('PARTIAL NOTATION STUDY | provisional pulse and meter | musical review pending',36,y-1,9)
            c.saveState();clip=c.beginPath();clip.rect(36,510-scale*(end-start),720,scale*(end-start));c.clipPath(clip,stroke=0,fill=0)
            c.translate(36,510-scale*(float(root.attrib['height'])-start))
            c.scale(scale,scale);renderPDF.draw(drawing,c,0,0);c.restoreState()
            line('Unmapped syllables/pitches are listed on the listening page. Original full audio is unchanged.',36,51,8)
            footer(792,'Native REEL notation study');pages.append(dict(kind='notation',song=title,svg_y=[start,end]));c.showPage()
        audio=checked(entry['audio']);guide=checked(entry['guide'])
        def rel(path):return Path(path).resolve().relative_to(output.resolve()).as_posix()
        rows=[];audio_id='audio-'+str(index)
        for number,text in enumerate(all_lines,1):
            phrases=[p for p in evidence['phrases'] if p['line']==number]
            syll=[s for s in evidence['syllables'] if s['line']==number]
            buttons=' '.join(f'<button data-audio="{audio_id}" data-start="{max(0,p["source_span_seconds"][0]-.6):.4f}" data-end="{p["source_span_seconds"][1]+.6:.4f}">{p["source_span_seconds"][0]:.1f}s</button>' for p in phrases)
            gaps=' · '.join(s['text'] for s in syll if not s['note_ids'])
            status='Accepted omission; retained for future rendition' if number in omitted else f'{sum(bool(s["note_ids"]) for s in syll)}/{len(syll)} syllable proposals overlap recovered notes'
            rows.append(f'<tr><td>{number}</td><td>{html.escape(text)}</td><td>{buttons}<p>{html.escape(status)}</p>'+('<p class="gap">Unmapped: '+html.escape(gaps)+'</p>' if gaps else '')+'</td></tr>')
        links=' · '.join(f'<a href="{html.escape(rel(checked(entry[role])),quote=True)}">{label}</a>' for role,label in [('musicxml','Editable MusicXML'),('midi','MIDI'),('svg','Native SVG study')])
        sections.append(f'<section><h2>{html.escape(title)}</h2><p>{html.escape(entry["edition"])} · {html.escape(entry["duration"])}</p><p>{html.escape(entry["status"])}</p><audio id="{audio_id}" controls preload="metadata" src="{rel(audio)}"></audio><details><summary>Partial notation diagnostic guide</summary><audio controls preload="none" src="{rel(guide)}"></audio><p>Simple oscillator guide; unmapped passages are silent.</p></details><p>{links}</p><table><tr><th>Line</th><th>Complete canonical poem</th><th>Listen / study gaps</th></tr>'+''.join(rows)+'</table></section>')
    c.save()
    page=output/'Listen and review.html'
    page.write_text('''<!doctype html><html lang="es"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Full Golden Songbook</title><style>body{font:17px system-ui;max-width:1050px;margin:32px auto;padding:0 18px;background:#f6f5ee;color:#24342e}section{background:white;padding:22px;margin:25px 0;border-radius:10px}audio{width:100%}td,th{text-align:left;vertical-align:top;padding:10px;border-bottom:1px solid #ddd}button{padding:7px 12px;margin:3px;cursor:pointer}.gap{background:#fff0c4;padding:8px}a{color:#14624b}table{width:100%;border-collapse:collapse}</style><h1>Four full Goldens</h1><p>Private working songbook. Producer-applied notation and timing proposals; musical review pending.</p><p><a href="Full%20Golden%20Songbook%20-%20Working%20Edition.pdf">Open printable songbook</a></p><p>Original full recordings and complete canonical poems are preserved. Forced expected-text timing does not establish that every word is independently recognized. Pitch, rhythm, meter, repeated-note articulation and final engraving remain open.</p>'''+''.join(sections)+'''<script>document.querySelectorAll('button[data-audio]').forEach(b=>b.addEventListener('click',()=>{document.querySelectorAll('audio').forEach(a=>a.pause());const a=document.getElementById(b.dataset.audio);a.currentTime=Number(b.dataset.start);a.dataset.stop=b.dataset.end;a.play().catch(e=>alert(e.message));}));document.querySelectorAll('audio').forEach(a=>a.addEventListener('timeupdate',()=>{if(a.dataset.stop&&a.currentTime>=Number(a.dataset.stop)){a.pause();delete a.dataset.stop;}}));</script></html>''',encoding='utf-8',newline='\n')
    doc=pymupdf.open(pdf);renderdir=output/'render-check';renderdir.mkdir(exist_ok=True)
    checks=[]
    for i,p in enumerate(doc):
        pix=p.get_pixmap(matrix=pymupdf.Matrix(1.3,1.3));pix.save(renderdir/f'page-{i+1:02}.png')
        text=p.get_text()
        if '\ufffd' in text:raise ValueError('Replacement glyph in PDF')
        checks.append(dict(page=i+1,width=p.rect.width,height=p.rect.height,characters=len(text),rendered=True))
    receipt=dict(schema='reel.local-songbook-review.v1',native_notation_modified=False,musical_approval=False,producer_applied=True,pages=pages,pdf_checks=checks,visual_review='Rendered pages retained; no human visual or listening approval is implied',outputs=[str(pdf),str(page)])
    (output/'review-receipt.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    return receipt


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('request',type=Path);p.add_argument('output',type=Path);args=p.parse_args()
    print(json.dumps(produce(json.loads(args.request.read_text(encoding='utf-8')),args.output),ensure_ascii=True))


if __name__=='__main__':main()
