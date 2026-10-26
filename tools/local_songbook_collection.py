"""Private bilingual reading fronts and a collection of existing score PDFs.

No musical notation is rendered or corrected here. Retained score pages are
copied unchanged and checked by rendered pixels; current source locks and
unresolved musical findings remain explicit in the collection manifest.
"""
from itertools import zip_longest
from pathlib import Path
import hashlib
import html
import json
import zipfile
from local_song_backend import checked, descriptor, offline, read, write
from local_instrumental_engrave import text_bounds


def paired_lines(pairs):
    rows = []
    for pair in pairs:
        rows.extend(zip_longest(pair['es'].splitlines(), pair['en'].splitlines(), fillvalue=''))
        rows.append(('', ''))
    return rows


def scoped_path(root, relative):
    result = (root/relative).resolve()
    if not result.is_relative_to(root.resolve()) or result == root.resolve():
        raise ValueError('Collection output escapes its private directory')
    return result


def main():
    offline()
    q = read('collection.json')
    if q['schema'] != 'reel.songbook-collection.v1' or q['permissions'] != dict(private_only=True, external_upload=False, public_release=False):
        raise ValueError('Explicit private collection request required')
    ids = [e['id'] for e in q['entries']]
    if not ids or len(ids) != len(set(ids)) or len(ids) > 100:
        raise ValueError('Collection needs unique bounded entries')
    for item in q['runtime_files']:
        checked(item['path'], item)
    from reportlab.pdfbase import pdfmetrics
    from reportlab.pdfbase.ttfonts import TTFont
    from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer, LongTable, TableStyle
    from reportlab.lib.styles import ParagraphStyle
    from reportlab.lib import colors
    import pymupdf as fitz
    font = checked(q['font']['path'], q['font'])
    pdfmetrics.registerFont(TTFont('CollectionText', str(font)))
    ink = colors.HexColor('#283f3b')
    small = ParagraphStyle('small', fontName='CollectionText', fontSize=9, leading=13, textColor=ink)
    body = ParagraphStyle('body', parent=small, fontSize=11, leading=16)
    title = ParagraphStyle('title', parent=body, fontSize=23, leading=29, spaceAfter=15)
    def paragraph(text, style=body):
        return Paragraph(html.escape(text).replace('\n', '<br/>'), style)
    def footer(canvas, doc):
        canvas.setStrokeColor(colors.HexColor('#b6a27a')); canvas.line(48, 39, 564, 39)
        canvas.setFillColor(ink); canvas.setFont('CollectionText', 8)
        canvas.drawString(48, 26, q['footer']); canvas.drawRightString(564, 26, str(doc.page))
    def build(path, story):
        SimpleDocTemplate(str(path), pagesize=(612, 792), leftMargin=48, rightMargin=48,
                          topMargin=48, bottomMargin=52, title=q['title'],
                          author=q['creator']).build(story, onFirstPage=footer, onLaterPages=footer)
    root = Path('collection'); root.mkdir(exist_ok=True)
    story = [paragraph(q['title'], title), paragraph(q['subtitle']), Spacer(1, 15),
             paragraph(q['notice'], small), Spacer(1, 16)]
    for i, entry in enumerate(q['entries'], 1):
        story.append(paragraph(f'{i}. {entry["title"]}', small))
    build(root/'contents.pdf', story)
    combined = fitz.open(root/'contents.pdf'); result = []; toc = []; sections = []
    for e in q['entries']:
        source = checked(e['score']['path'], e['score']); source_pdf = fitz.open(source)
        front = scoped_path(root, 'fronts/'+e['id']+'.pdf'); front.parent.mkdir(parents=True, exist_ok=True)
        story = [paragraph(e['title'], title), paragraph(e['edition']+' | '+e['duration'], small),
                 paragraph(e['status'], small), Spacer(1, 12)]
        if e.get('attribution'):
            story += [paragraph(e['attribution'], small), Spacer(1, 12)]
        rows = [[paragraph('Español', small), paragraph('English', small)]]
        rows += [[paragraph(es), paragraph(en)] for es, en in paired_lines(e['pairs'])]
        table = LongTable(rows, colWidths=[250, 250], repeatRows=1, hAlign='LEFT')
        table.setStyle(TableStyle([('VALIGN',(0,0),(-1,-1),'TOP'),
                                   ('LEFTPADDING',(0,0),(-1,-1),0),
                                   ('RIGHTPADDING',(0,0),(-1,-1),12),
                                   ('TOPPADDING',(0,0),(-1,-1),3),
                                   ('BOTTOMPADDING',(0,0),(-1,-1),3),
                                   ('LINEBELOW',(0,0),(-1,0),.5,colors.HexColor('#b6a27a'))]))
        story.append(table)
        if e.get('limitations'):
            story += [Spacer(1, 12), paragraph(e['limitations'], small)]
        build(front, story)
        doc = fitz.open(front); front_pages = len(doc); doc.insert_pdf(source_pdf)
        target = scoped_path(root, 'scores/'+e['id']+'.pdf'); target.parent.mkdir(parents=True, exist_ok=True)
        doc.save(target, deflate=True, garbage=0)
        # Independent copying QA: no changes to any source score page.
        actual = fitz.open(target)
        for i, page in enumerate(source_pdf):
            if page.get_pixmap(alpha=False).samples != actual[front_pages+i].get_pixmap(alpha=False).samples:
                raise ValueError('Collection changed retained score pixels: '+e['id'])
        toc.append([1,e['title'],len(combined)+1]); combined.insert_pdf(actual)
        result.append(dict(id=e['id'],title=e['title'],pdf=descriptor(target),
                           source_score=e['score'],source_lock=e['source_lock'],
                           source_score_pages_unchanged=True,reading_pages=front_pages,
                           score_pages=len(source_pdf),status=e['status']))
        links = dict(e.get('links',{}),pdf='scores/'+e['id']+'.pdf')
        buttons = ' · '.join('<a href="'+html.escape(url,quote=True)+'">'+html.escape(label)+'</a>' for label,url in links.items() if label!='audio')
        text = ''.join('<div class="pair"><pre>'+html.escape(p['es'])+'</pre><pre lang="en">'+html.escape(p['en'])+'</pre></div>' for p in e['pairs'])
        audio = '<audio controls preload="none" src="'+html.escape(links['audio'],quote=True)+'"></audio>' if links.get('audio') else ''
        sections.append('<section id="'+html.escape(e['id'],quote=True)+'"><h2>'+html.escape(e['title'])+'</h2><p>'+html.escape(e['edition']+' · '+e['duration'])+'</p><p>'+html.escape(e['status'])+'</p>'+audio+'<p>'+buttons+'</p><details><summary>Lectura bilingüe</summary>'+text+'</details><p>'+html.escape(e.get('limitations',''))+'</p></section>')
    combined.set_toc(toc); combined.save(root/'complete-songbook.pdf', deflate=True, garbage=0)
    qa = []
    with zipfile.ZipFile('collection-rendered-qa.zip','w',zipfile.ZIP_DEFLATED) as archive:
        for i,page in enumerate(combined):
            pix=page.get_pixmap(alpha=False); outside,overhang=text_bounds(page.get_text('dict')['blocks'],page.rect.width,page.rect.height)
            if outside:
                raise ValueError('Collection text extends past a page')
            data=pix.samples; ink=sum(v<240 for v in data)/len(data)
            if ink<.0001:
                raise ValueError('Collection contains a blank page')
            name='page-'+str(i+1)+'.png'; path=root/name; pix.save(path); archive.write(path,name)
            qa.append(dict(page=i+1,render=descriptor(path),ink_fraction=ink,text_outside_page=outside,musical_font_metric_overhangs=overhang))
    page='<!doctype html><html lang="es"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>'+html.escape(q['title'])+'</title><style>body{font:18px/1.6 Georgia,serif;max-width:1100px;margin:28px auto;padding:0 18px;background:#f2e8d3;color:#283f3b}h1,h2{font-weight:normal}section{background:#fffaf0;border:1px solid #c3ae87;padding:22px;margin:22px 0}audio{width:100%}.pair{display:grid;grid-template-columns:1fr 1fr;gap:24px}pre{font:inherit;white-space:pre-wrap}a{color:#00685d}@media(max-width:650px){.pair{grid-template-columns:1fr}}summary{cursor:pointer}</style><h1>'+html.escape(q['title'])+'</h1><p>'+html.escape(q['notice'])+'</p><p><a href="complete-songbook.pdf">Libro completo / Complete book</a></p>'+''.join(sections)+'</html>'
    (root/'index.html').write_text(page,encoding='utf-8',newline='\n')
    write('collection-check.json',dict(schema='reel.songbook-collection-check.v1',entries=result,
          combined=descriptor(root/'complete-songbook.pdf'),rendered_pages=qa,
          source_scores_modified=False,producer_applied=True,
          human_musical_approval=False,human_visual_approval=False))
    print('COLLECTION VERIFIED',len(result),'entries',len(combined),'pages; original score pixels preserved',flush=True)


if __name__=='__main__':
    main()
