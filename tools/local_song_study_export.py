"""Native validation/export of an explicit, previously constructed study model."""
import os
from pathlib import Path
import subprocess
from local_song_backend import checked, offline, read, write


def main():
    offline();request=read('export.json')
    if request['schema']!='reel.local-song-study-export-request.v1':raise ValueError('Unsupported export request')
    checked('model.json',request['model']);checked('expected-singing.txt',request['expected_singing'])
    executable=checked(request['native_runtime']['path'],request['native_runtime'])
    def native(command,*args):
        import json
        result=subprocess.run([str(executable),command,*map(str,args),'--output','json'],capture_output=True,text=True,encoding='utf-8')
        if result.returncode:raise RuntimeError(result.stderr)
        return json.loads(result.stdout)
    checks=dict(model=native('music-model-validate','model.json'))
    native('music-score-export-plan','model.json','--output-path','score-plan.json')
    native('music-score-export-render','score-plan.json','model.json','--output-dir','rendered')
    checks['score_export']=native('music-score-export-check','rendered/receipt.json','score-plan.json','model.json','rendered')
    write('native-checks.json',checks)
    for filename in ['score.mid','score.musicxml','lead-sheet.svg','rehearsal-guide.wav','receipt.json']:os.link(Path('rendered')/filename,filename)
    print('NATIVE STAGED STUDY EXPORT VERIFIED',flush=True)


if __name__=='__main__':main()
