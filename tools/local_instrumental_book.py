"""Private bilingual instrumental working books from audited engraved scores."""
from pathlib import Path
import html
import zipfile
from local_song_backend import checked,descriptor,offline,read,write


def main():
    offline();q=read('book-request.json')
    if q['schema']!='reel.instrumental-book-request.v1' or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Private book request required')
    for item in q['runtime_files']:checked(item['path'],item)
    from reportlab.pdfbase import pdfmetrics
    from reportlab.pdfbase.ttfonts import TTFont
    from reportlab.platypus import SimpleDocTemplate,Paragraph,Spacer
    from reportlab.lib.styles import ParagraphStyle
    import pymupdf as fitz
    font=checked(q['font']['path'],q['font']);pdfmetrics.registerFont(TTFont('BookText',str(font)))
    body=ParagraphStyle('body',fontName='BookText',fontSize=11,leading=16,spaceAfter=14,textColor='#263a43')
    title=ParagraphStyle('title',parent=body,fontSize=25,leading=31,spaceAfter=24)
    small=ParagraphStyle('small',parent=body,fontSize=9,leading=13,textColor='#53636a')
    def cover(path,heading,paragraphs):
        story=[Paragraph(html.escape(heading),title),Spacer(1,12)]
        for text,style in paragraphs:story.append(Paragraph(html.escape(text),style))
        SimpleDocTemplate(str(path),pagesize=(612,792),leftMargin=54,rightMargin=54,topMargin=60,bottomMargin=54,title=heading,author=q['creator']).build(story)
    notice='Edición de trabajo para revisión musical. La transcripción, el pulso, el compás y las voces de lectura son propuestas.'
    english='Working edition for musical review. Transcription, pulse, meter and reading voices are proposals.'
    output=Path('book');output.mkdir(exist_ok=True);combined=fitz.open();cover(output/'contents.pdf',q['title'],[(q['subtitle'],body),(' / '.join(e['title'] for e in q['entries']),body),(notice,small),(english,small)]);combined.insert_pdf(fitz.open(output/'contents.pdf'));rows=[];toc=[]
    for index,e in enumerate(q['entries']):
        checked(e['score_path'],e['score']);paragraphs=[('Escena',small),(e['es'],body),('Scene',small),(e['en'],body),(f"Grabación original / Original recording: {e['seconds']//60}:{e['seconds']%60:02d}",small),(notice,small),(english,small)]
        if e.get('limitations'):paragraphs.append((e['limitations'],small))
        folder=output/e['slug'];folder.mkdir(exist_ok=True);cover(folder/'notes.pdf',e['title'],paragraphs);doc=fitz.open();doc.insert_pdf(fitz.open(folder/'notes.pdf'));doc.insert_pdf(fitz.open(e['score_path']));target=folder/'score.pdf';doc.save(target,garbage=0,deflate=True);toc.append([1,e['title'],len(combined)+1]);combined.insert_pdf(doc);rows.append(dict(slug=e['slug'],pages=len(doc),pdf=descriptor(target),source_score=e['score'],source_golden_lock=e['golden_lock'],musical_approval=False))
    combined.set_toc(toc);combined.save(output/'instrumentales-del-libro.pdf',garbage=0,deflate=True)
    qa=[]
    with zipfile.ZipFile('book-rendered-qa.zip','w',compression=zipfile.ZIP_DEFLATED) as archive:
        for index,page in enumerate(combined):
            pix=page.get_pixmap(matrix=fitz.Matrix(1,1),alpha=False);path=output/('qa-'+str(index+1)+'.png');pix.save(path);archive.write(path,path.name);qa.append(dict(page=index+1,width=page.rect.width,height=page.rect.height,render=descriptor(path)))
    write('book-check.json',dict(entries=rows,combined=descriptor(output/'instrumentales-del-libro.pdf'),pages=qa,producer_applied=True,human_visual_approval=False,musical_approval=False,source_recordings_unchanged=True));print('BILINGUAL INSTRUMENTAL WORKING BOOK ASSEMBLED',len(rows),'entries',len(combined),'pages',flush=True)


if __name__=='__main__':main()
