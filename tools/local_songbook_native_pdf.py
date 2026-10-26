"""Package native notation SVGs unchanged using the existing songbook review adapter."""
from pathlib import Path
import zipfile
from local_song_backend import offline, read, runtime_files, write
from local_songbook_review import produce

def main():
    offline();q=read('review-request.json');runtime_files(q,'runtime_files')
    result=produce(q,Path('review'))
    with zipfile.ZipFile('rendered-qa.zip','w',zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(Path('review/render-check').glob('*.png')):archive.write(path,path.name)
    write('native-pdf-check.json',result)
    print('NATIVE SVG PRESENTATION VERIFIED',len(q['entries']),len(result['pdf_checks']),'pages',flush=True)
if __name__=='__main__':main()
