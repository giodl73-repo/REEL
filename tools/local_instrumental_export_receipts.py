"""Reproduce native reading-score receipts without changing retained score bytes."""
import os
from pathlib import Path
from local_song_backend import checked,read,write,offline,descriptor
from local_song_piano_arrangement import native


def main():
    offline();q=read('receipt-request.json');root=Path.cwd()
    if q['schema']!='reel.instrumental-export-receipts.v1' or not 1<=len(q['items'])<=14 or q['permissions']!=dict(private_only=True,external_upload=False,public_release=False):raise ValueError('Explicit bounded private score receipt request required')
    for item in q['items']:
        directory=item['directory']
        if Path(directory).name!=directory or directory in ['.','..']:raise ValueError('Invalid receipt case directory')
        os.chdir(root/directory)
        try:
            checked('reading-model.json',item['model']);native(q,'music-score-export-plan','reading-model.json','--output-path','reading-plan.json');native(q,'music-score-export-render','reading-plan.json','reading-model.json','--output-dir','rendered');result=native(q,'music-score-export-check','rendered/receipt.json','reading-plan.json','reading-model.json','rendered')
            for path,asset in item['expected_outputs'].items():checked(Path('rendered')/path,asset)
            actual=descriptor('rendered/receipt.json')
            if actual['sha256']!=item['original_receipt_sha256']:raise ValueError('Reproduced native receipt differs from original verified receipt hash')
            Path('receipt.json').hardlink_to('rendered/receipt.json')
            write('receipt-reproduction-check.json',dict(native_export_check=result,retained_model=item['model'],reproduced_receipt=actual,original_receipt_sha256=item['original_receipt_sha256'],existing_export_bytes_unchanged=True,verified_existing_outputs=item['expected_outputs'],method='Same pinned native exporter, retained model/source/analyses and exact original relative paths; all retained XML/MIDI/guide hashes and original receipt hash matched',musical_approval=False))
        finally:os.chdir(root)
        print('EXACT NATIVE SCORE RECEIPT REPRODUCED',directory,flush=True)


if __name__=='__main__':main()
