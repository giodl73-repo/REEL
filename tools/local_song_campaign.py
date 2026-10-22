"""Serial private song auditions through the existing Rust asset/job contract.

All asset admission, registry, versioning and execution delegate to reel-assets.
The campaign is explicit project data, never discovered from a repository.
"""
from __future__ import annotations

import argparse
import html
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

from local_song_backend import read, digest, descriptor, checked, format_lyrics, format_repeated_sections


def guide_targets(slots, request, targets):
    selected=sorted(set(targets))
    by_id={slot['id']:slot for slot in slots}
    if not selected or any(target not in by_id for target in selected):
        raise ValueError('Audio guide targets must name campaign slots')
    for target in selected:
        slot=by_id[target]
        if slot['source']!=request['source'] or slot['source_lock']!=request['source_lock']:
            raise ValueError('Audio guide target belongs to another source/version')
    return selected


def revised_caption(slot, briefs):
    revision=briefs.get(slot['id'])
    if revision is None:return slot['caption']
    if not isinstance(revision,dict):raise ValueError('Musical brief revision must be an object')
    if set(revision)-{'source','source_lock','caption','repeat_section_lines','phrase_max_words'}:
        raise ValueError('Unknown musical brief revision field')
    if revision.get('source')!=slot['source'] or revision.get('source_lock')!=slot['source_lock']:
        raise ValueError('Musical brief revision belongs to another source/version')
    caption=revision.get('caption')
    if not isinstance(caption,str) or not caption.strip() or len(caption.encode('utf-8'))>4096:
        raise ValueError('Musical brief revision must contain a bounded caption')
    if 'repeat_section_lines' in revision and (type(revision['repeat_section_lines']) is not int or revision['repeat_section_lines'] not in [0,*range(2,9)]):
        raise ValueError('Unsupported scoped repeated-section layout')
    if 'phrase_max_words' in revision and (type(revision['phrase_max_words']) is not int or not 4<=revision['phrase_max_words']<=16):
        raise ValueError('Unsupported scoped phrase size')
    return caption


class Campaign:
    def __init__(self, root, assets, work, metadata_retries=0):
        self.root=Path(root).resolve()
        self.assets=Path(assets).resolve()
        self.work=Path(work).resolve()
        self.work.mkdir(parents=True,exist_ok=True)
        self.metadata_retries=metadata_retries
        self.repeat_after_failures=None
        self.planner_asset=None
        self.repeat_section_lines=None
        self.phrase_max_words=None
        self.asr_dtype=None
        self.timing_reference=None
        self.lyric_focus=None
        self.ctc_runtime=None
        self.audio_guide=None
        self.brief_asset=None
        self.briefs={}

    def call(self, command, *args, env=None):
        for attempt in range(self.metadata_retries+1):
            result=subprocess.run([str(self.assets),'--root',str(self.root),command,*map(str,args)],capture_output=True,text=True,encoding='utf-8',env=env)
            retryable=command in ['ingest','register','register-version','pin','select','fetch-existing'] or (command=='run-job' and 'Job not started; admission:' in result.stderr)
            if result.returncode and 'Storage metadata is in use;' in result.stderr and retryable and attempt < self.metadata_retries:
                print('Metadata busy; explicit bounded retry',command,attempt+1,flush=True)
                time.sleep(min(2,.25*(attempt+1)))
                continue
            break
        if result.returncode:
            raise RuntimeError(result.stderr.strip() or result.stdout.strip())
        return json.loads(result.stdout)

    def save(self, name, data):
        path=self.work/name
        path.parent.mkdir(parents=True,exist_ok=True)
        text=json.dumps(data,ensure_ascii=False,indent=2)+'\n'
        temp=path.with_suffix(path.suffix+'.tmp')
        temp.write_text(text,encoding='utf-8',newline='\n')
        os.replace(temp,path)
        return path

    def register(self, assets, provenance):
        rows=[]
        for asset in assets:
            rows.append(dict(sha256=asset['sha256'],bytes=asset['bytes'],provenance=provenance,
                rights='Private owner-authorized local production; source-specific tool/model licenses remain authoritative; no external transfer or release',
                selection_state='private-unselected-audition'))
        self.call('register',self.save(f'registrations/registration-{time.time_ns()}.json',dict(assets=rows)))

    def ingest(self, path, provenance):
        asset=self.call('ingest',path)
        self.register([asset],provenance)
        return {k:asset[k] for k in ['sha256','bytes','cache_uri']}

    def reuse_or_ingest(self, path, provenance):
        """Reuse freshly verified registered bytes; admission still gates new data.

        This changes no existing registration/provenance or cache policy. A
        corrupt active object fails instead of falling back to a fresh ingest.
        """
        asset=descriptor(path)
        asset['cache_uri']='cache://sha256/'+asset['sha256']
        try:
            self.locate(asset)
        except RuntimeError as error:
            if not any(reason in str(error) for reason in ['Unregistered hash','Named object absent from active caches; archive not probed']):
                raise
            return self.ingest(path,provenance)
        return asset

    def asset_json(self, name, data, provenance):
        return self.ingest(self.save(name,data),provenance)

    def locate(self, asset):
        row=self.call('verify-active',asset['sha256'])
        if row['bytes'] != asset['bytes']:
            raise ValueError('Asset size differs from frozen reference')
        return Path(row['path'])

    def freeze_runtime(self, args):
        ace=Path(args.ace_root).resolve()
        if subprocess.check_output(['git','-C',str(ace),'status','--porcelain'],text=True).strip():
            raise ValueError('ACE source checkout must be clean')
        commit=subprocess.check_output(['git','-C',str(ace),'rev-parse','HEAD'],text=True).strip()
        model_files=[]
        for folder in ['acestep-v15-turbo','Qwen3-Embedding-0.6B','vae']:
            model_files.extend(p for p in (ace/'checkpoints'/folder).rglob('*') if p.is_file() and '.cache' not in p.parts)
        for required in ['acestep-v15-turbo/model.safetensors','Qwen3-Embedding-0.6B/model.safetensors','vae/diffusion_pytorch_model.safetensors']:
            if not (ace/'checkpoints'/required).is_file():raise FileNotFoundError(required)
        code_names=subprocess.check_output(['git','-C',str(ace),'ls-files','acestep'],text=True).splitlines()
        code=[ace/name for name in code_names if name.endswith('.py')]
        def rows(paths):return [dict(path=str(p.resolve()),**descriptor(p)) for p in sorted(paths)]
        whisper=Path(args.whisper_root).resolve()
        analysis=[p for p in whisper.iterdir() if p.is_file()]
        analysis.append(Path(args.demucs).resolve())
        # Freeze cache-backed model custody, while using existing installations
        # read-only rather than copying multi-GB models into every job workspace.
        custody=[]
        progress_path=self.work/'snapshot-progress.json'
        progress=read(progress_path) if progress_path.exists() else {}
        for path in model_files+analysis+[Path(args.python),Path(args.ffmpeg)]:
            print('Freeze runtime asset',path.name,flush=True)
            key=str(path.resolve())
            identity=dict(size=path.stat().st_size,mtime_ns=path.stat().st_mtime_ns)
            if key in progress and progress[key]['identity']==identity:
                # The previous explicit ingest already freshly hashed both
                # installation and CAS. Resume only the same installation file.
                asset=progress[key]['asset']
                self.call('locate',asset['sha256'])
            else:
                asset=self.ingest(path,'Explicit installed local music/analysis runtime input')
                progress[key]=dict(identity=identity,asset=asset)
                self.save('snapshot-progress.json',progress)
            custody.append(asset)
        archive=self.work/'ace-source.zip'
        if not archive.exists():
            with archive.open('xb') as stream:subprocess.run(['git','-C',str(ace),'archive','--format=zip',commit],stdout=stream,check=True)
        code_asset=self.ingest(archive,'Exact public ACE source archive at '+commit)
        packages=subprocess.check_output([args.python,'-c','import importlib.metadata,json,sys; print(json.dumps({"python":sys.version,"packages":sorted((d.metadata["Name"],d.version) for d in importlib.metadata.distributions())}))'],text=True)
        runtime=dict(schema='reel.local-song-runtime.v1',ace_root=str(ace),ace_commit=commit,
            python=dict(path=str(Path(args.python).resolve()),**descriptor(args.python)),
            ffmpeg=dict(path=str(Path(args.ffmpeg).resolve()),**descriptor(args.ffmpeg)),
            generation_files=rows(model_files+code),analysis_files=rows(analysis),
            whisper_root=str(whisper),demucs_path=str(Path(args.demucs).resolve()),
            environment=json.loads(packages),custody=custody,ace_source_archive=code_asset)
        asset=self.asset_json('runtime.json',runtime,'Frozen local offline music/analysis runtime identity')
        logical='tooling/local-song-runtime'
        vid='runtime-'+asset['sha256'][:16]
        package=dict(schema='caminos.artifact-package.v1',artifact_id=logical,kind='tool-runtime',version_id=vid,
            assets=dict({'runtime':asset,'ace-source':code_asset},**{f'input/{i:03d}':row for i,row in enumerate(custody)}),public_release=False)
        package_asset=self.asset_json('runtime-package.json',package,'Immutable local music runtime package')
        version=dict(logical_id=logical,version_id=vid,manifest_sha256=package_asset['sha256'],
            payloads=sorted({row['sha256'] for row in package['assets'].values()}),dependencies=[])
        self.call('register-version',self.save('runtime-version.json',version))
        self.call('pin',logical,vid)
        asset['version_lock']={k:version[k] for k in ['logical_id','version_id','manifest_sha256']}
        self.save('runtime-asset.json',asset)
        return asset

    def request(self, slot, attempt):
        source=self.locate(slot['source'])
        lyrics=source.read_bytes().decode('utf-8')
        caption=revised_caption(slot,self.briefs)
        brief=self.briefs.get(slot['id'],{})
        phrase_words=brief.get('phrase_max_words',self.phrase_max_words)
        section_lines=brief.get('repeat_section_lines',self.repeat_section_lines)
        # Canonical words stay immutable under every explicit rendering policy.
        render=format_lyrics(lyrics,join_single_word_continuations=attempt>1,max_phrase_words=phrase_words)
        duration=min(480,slot['duration_seconds']+15*((attempt-1)//3))
        ceiling=slot.get('max_duration_seconds')
        if ceiling is not None and (isinstance(ceiling,bool) or not isinstance(ceiling,(int,float)) or not slot['duration_seconds'] <= ceiling <= 480):
            raise ValueError('Duration ceiling must be between the initial duration and 480 seconds')
        repetition={}
        if slot['id'] in self.briefs:repetition['musical_brief_revision']=dict(asset=self.brief_asset,slot_id=slot['id'])
        if self.asr_dtype:repetition['recognition_dtype']=self.asr_dtype
        if self.ctc_runtime:repetition['independent_ctc_check']=dict(component=self.ctc_runtime['component'],version_lock=self.ctc_runtime['version_lock'],max_failed_lines=3,canonical_prompt=False)
        if attempt>1 or self.timing_reference:
            caption+=(' Syllabic melody, clear Spanish diction and final consonants; minimal melisma.' if phrase_words else ' Clear natural articulation of every supplied word, including gentle but distinct final consonants and plural s; keep the melody expressive.')
            repetition['render_format']='Single-word continuations joined to their preceding phrase within each original stanza; source words and canonical line breaks unchanged'
        if phrase_words:
            repetition.update(phrase_max_words=phrase_words,render_format='Consecutive source fragments joined into bounded sung phrases; original stanza boundaries, source words and canonical lines unchanged')
        if section_lines:
            render=format_repeated_sections(lyrics,section_lines,phrase_words)
            duration=min(480,max(duration,5*round(slot['duration_seconds']*1.75/5)))
            caption+=(' Sing each supplied stanza copy verbatim; no invented chorus.' if phrase_words else ' Sing each supplied short stanza twice before continuing to the next stanza. Both copies are supplied; keep every original word, melodic phrasing and clear diction. No invented chorus.')
            repetition.update(performance_repetitions=2,repeat_section_lines=section_lines,
                repeat_authority='Owner-authorized repeated lines; each unchanged consecutive source section supplied twice')
        if self.planner_asset:
            repetition['semantic_planner']=self.planner_asset
            repetition['render_format']='Unchanged source sections repeated twice' if section_lines else 'Single unchanged source pass, single-word continuations joined; optional frozen local semantic plan'
        elif not section_lines and self.repeat_after_failures is not None and attempt>self.repeat_after_failures:
            # The operator must explicitly authorize repetitions. Repeat exact
            # complete source text, never synthesize replacement lyric lines.
            body=render.removesuffix('\n[Outro - instrumental]')
            render=body+'\n'+body+'\n[Outro - instrumental]'
            duration=min(480,max(duration,5*round(slot['duration_seconds']*1.75/5)))
            caption+=' Sing the complete supplied poem, then its supplied second complete pass. Keep both passes melodic and clear; no invented words.'
            repetition.update(performance_repetitions=2,repeat_authority='Explicit campaign permission for repeated lines; unchanged complete poem supplied twice')
        seed=slot['seed_base']+attempt
        if self.timing_reference:
            ref=self.timing_reference
            if ref['source']!=slot['source'] or ref['source_lock']!=slot['source_lock']:
                raise ValueError('Timing reference belongs to another source/version')
            offset=attempt-ref['attempt_starts'][slot['id']]
            if offset<0:raise ValueError('Timing reference cannot rewrite prior requests')
            seed=ref['seed']+offset;duration=ref['duration_seconds']
            repetition['timing_seed_reference']=dict(request=ref['request'],version_lock=ref['version_lock'],seed_offset=offset,audio_conditioning=False)
        if self.lyric_focus:
            lines=[line for line in lyrics.splitlines() if line.strip()]
            focused=' / '.join(lines[i-1].strip() for i in self.lyric_focus['line_numbers'])
            caption="Clearly sing this supplied lyric phrase: '"+focused+"'. "+caption
            repetition['lyric_focus']=self.lyric_focus
        task='text2music'
        guide=self.audio_guide
        if guide and guide.get('target_slot_ids') and slot['id'] not in guide['target_slot_ids']:
            guide=None
        if guide:
            original=read(self.locate(guide['request']))
            if original['task'] not in ('text2music','cover') or original['canonical_lyrics']!=lyrics or original['source_lock']!=slot['source_lock']:
                raise ValueError('Audio guide must be generated from the identical poem version')
            if self.planner_asset:raise ValueError('Generated-guide cover does not combine with optional planning')
            render=original['render_lyrics'];duration=original['duration_seconds'];task='cover'
            for key in ['performance_repetitions','repeat_section_lines','phrase_max_words','repeat_authority']:
                repetition.pop(key,None)
                if key in original:repetition[key]=original[key]
            repetition.update(guide_audio=guide['recording'],guide_request=guide['request'],guide_generation=guide['generation'],
                guide_version_lock=guide['version_lock'],guide_label=guide['label'],audio_cover_strength=guide['strength'],
                render_format='Exact supplied render layout of the generated same-poem guide; source words unchanged')
            if guide.get('short_edit'):
                edition=read(self.locate(guide['short_edit']['recipe']))
                render=format_lyrics(lyrics,join_single_word_continuations=True,max_phrase_words=phrase_words)
                duration=edition['duration_seconds']
                for key in ['performance_repetitions','repeat_section_lines','repeat_authority']:
                    repetition.pop(key,None)
                repetition.update(guide_short_edit=guide['short_edit'],render_format='Single unchanged poem pass guided by an explicitly verified short edition')
            if 'noise_strength' in guide:repetition['cover_noise_strength']=guide['noise_strength']
        if ceiling is not None:
            if (guide or self.timing_reference) and duration>ceiling:
                raise ValueError('Reference duration exceeds the candidate duration ceiling')
            duration=min(duration,ceiling)
            repetition['max_duration_seconds']=ceiling
        return dict(schema='reel.local-song-request.v1',candidate_id=f"{slot['id']}-take-{attempt:03d}",
            slot_id=slot['id'],title=slot['title'],source=slot['source'],source_lock=slot['source_lock'],
            canonical_lyrics=lyrics,render_lyrics=render,caption=caption,duration_seconds=duration,
            seed=seed,task=task,language='es',
            permissions=dict(private_only=True,external_upload=False,identifiable_imitation=False,public_release=False),**repetition)

    def run_take(self, slot, attempt, runtime_asset, backend_asset):
        request=self.request(slot,attempt)
        rid=request['candidate_id']
        record_path=self.work/rid/'artifact.json'
        if record_path.exists():
            print('RESUME retained take',rid,flush=True)
            return self.summarize_take(slot,attempt,read(record_path),record_path)
        request_asset=self.asset_json(f'{rid}/request.json',request,'Exact submitted request for '+rid)
        runtime=read(self.locate(runtime_asset))
        def a(row):return {k:row[k] for k in ['sha256','bytes']}
        outputs={role:dict(path=path,max_bytes=maximum) for role,path,maximum in [
            ('recording','recording.wav',192*1024**2),('generation','generation.json',2*1024**2),
            ('generation-log','step-0.log',8*1024**2),('analysis-log','step-1.log',8*1024**2),
            ('listening','preview.mp3',24*1024**2),('vocal-estimate','vocals.wav',192*1024**2),
            ('lyric-analysis','analysis.json',8*1024**2),('transcripts','transcripts.zip',8*1024**2)]}
        inputs={'backend.py':a(backend_asset),'runtime.json':a(runtime_asset),'request.json':a(request_asset),'source.txt':a(slot['source'])}
        if request['task']=='cover':
            inputs.update({'guide.wav':a(self.audio_guide['recording']),'guide-request.json':a(self.audio_guide['request']),
                'guide-generation.json':a(self.audio_guide['generation'])})
            if self.audio_guide.get('short_edit'):
                inputs.update({'guide-edit.json':a(self.audio_guide['short_edit']['recipe']),
                    'guide-edit-receipt.json':a(self.audio_guide['short_edit']['receipt'])})
        operations=['generate','analyze']
        if self.planner_asset:
            inputs['planner.json']=a(self.planner_asset)
            outputs['semantic-plan']=dict(path='plan.json',max_bytes=2*1024**2)
            outputs['planning-log']=dict(path='step-0.log',max_bytes=8*1024**2)
            outputs['generation-log']['path']='step-1.log'
            outputs['analysis-log']['path']='step-2.log'
            operations.insert(0,'plan')
        if self.ctc_runtime:
            if a(self.ctc_runtime['backend'])!=a(backend_asset):raise ValueError('CTC and primary line-accounting backends differ')
            inputs.update({'ctc.py':a(self.ctc_runtime['script']),'local_song_backend.py':a(backend_asset),'component.json':a(self.ctc_runtime['component'])})
            outputs.update({'primary-lyric-analysis':dict(path='prior-analysis.json',max_bytes=8*1024**2),
                'ctc-transcript':dict(path='ctc-transcript.json',max_bytes=8*1024**2),'ctc-request':dict(path='ctc.json',max_bytes=2*1024**2),
                'ctc-log':dict(path=f'step-{len(operations)}.log',max_bytes=8*1024**2)})
        job=dict(budget_bytes=3*1024**3,scratch_bytes=1536*1024**2,
            inputs=inputs,
            tools={},installed_tools={'python':runtime['python']},
            steps=[dict(tool='python',args=['backend.py',operation],timeout_seconds=1800) for operation in operations],outputs=outputs)
        if self.ctc_runtime:job['steps'].append(dict(tool='python',args=['ctc.py','--automatic'],timeout_seconds=600))
        manifest=self.save(f'{rid}/job.json',job)
        manifest_asset=self.ingest(manifest,'Trusted Rust recording/analysis job for '+rid)
        self.save('active.json',dict(candidate_id=rid,stage='rust-job-running',manifest=manifest_asset,started=time.time()))
        print('START',rid,request['duration_seconds'],'seconds',flush=True)
        result=self.call('run-job',manifest)
        out={role:{k:row[k] for k in ['sha256','bytes','cache_uri']} for role,row in result['outputs'].items()}
        self.register(list(out.values()),'Retained original take and diagnostic evidence for '+rid)
        result_asset=self.asset_json(f'{rid}/job-result.json',result,'Rust job result for '+rid)
        analysis=read(self.locate(out['lyric-analysis']))
        package=dict(schema='caminos.artifact-package.v1',artifact_id='song-audition/'+slot['id'],kind='song',
            version_id=f'take-{attempt:03d}',edition='full',assets=dict(out,request=request_asset,runtime=runtime_asset,
            source=slot['source'],backend=backend_asset,job=manifest_asset,job_result=result_asset),
            dependencies=[slot['source_lock'],runtime_asset['version_lock']],selection_status='lyric-screen-qualified' if analysis['passed'] else 'lyric-screen-retry',
            human_listening='pending',public_release=False)
        if self.planner_asset:
            package['assets']['planner']=self.planner_asset
            package['dependencies'].append(self.planner_asset['version_lock'])
        if self.timing_reference:
            package['assets']['timing-reference-request']=self.timing_reference['request']
            package['dependencies'].append(self.timing_reference['version_lock'])
        if self.lyric_focus:
            package['assets']['prior-lyric-analysis']=self.lyric_focus['analysis']
            package['dependencies'].append(self.lyric_focus['version_lock'])
        if self.ctc_runtime:
            package['assets'].update({'ctc-component':self.ctc_runtime['component'],'ctc-script':self.ctc_runtime['script']})
            package['dependencies'].append(self.ctc_runtime['version_lock'])
        if request['task']=='cover':
            package['assets'].update({'guide-recording':self.audio_guide['recording'],'guide-request':self.audio_guide['request'],
                'guide-generation':self.audio_guide['generation']})
            package['dependencies'].append(self.audio_guide['version_lock'])
            if self.audio_guide.get('short_edit'):
                package['assets'].update({'guide-edit-recipe':self.audio_guide['short_edit']['recipe'],
                    'guide-edit-receipt':self.audio_guide['short_edit']['receipt']})
        if request.get('musical_brief_revision'):
            package['assets']['musical-brief-revision']=self.brief_asset
            package['dependencies'].append(self.brief_asset['version_lock'])
        package['dependencies']=list({(d['logical_id'],d['version_id'],d['manifest_sha256']):d for d in package['dependencies']}.values())
        package_asset=self.asset_json(f'{rid}/package.json',package,'Immutable song audition package for '+rid)
        version=dict(logical_id=package['artifact_id'],version_id=package['version_id'],manifest_sha256=package_asset['sha256'],
            payloads=sorted({row['sha256'] for row in package['assets'].values()}),dependencies=package['dependencies'])
        self.call('register-version',self.save(f'{rid}/version.json',version))
        # Protect every retained attempt, not only the qualifying take.
        self.call('pin',version['logical_id'],version['version_id'])
        record=dict(schema='caminos.artifact-record.v1',artifact_id=package['artifact_id'],kind='song',public_release=False,
            current_version=version['version_id'],versions=[dict(version_id=version['version_id'],edition='full',manifest=package_asset,
            version_lock={k:version[k] for k in ['logical_id','version_id','manifest_sha256']})])
        record_path=self.save(f'{rid}/artifact.json',record)
        return self.summarize_take(slot,attempt,record,record_path)

    def summarize_take(self,slot,attempt,record,record_path):
        version=record['versions'][0]
        self.call('artifact-verify',record_path,version['version_id'])
        package_asset=version['manifest']
        package=read(self.locate(package_asset))
        assets=package['assets']
        request_asset=assets['request']
        request=read(self.locate(request_asset))
        if request!=self.request(slot,attempt):raise ValueError('Retained request differs from the frozen slot')
        rid=request['candidate_id']
        analysis=read(self.locate(assets['lyric-analysis']))
        result=read(self.locate(assets['job_result']))
        out={role:assets[role] for role in ['recording','listening','generation','generation-log','analysis-log','vocal-estimate','lyric-analysis','transcripts']}
        summary=dict(id=rid,slot_id=slot['id'],attempt=attempt,qualified=analysis['passed'],
            exact_lines=analysis['exact_lines'],qualifying_lines=analysis['qualifying_lines'],total_lines=analysis['total_lines'],
            one_word_gap_lines=analysis['one_word_gap_lines'],manifest=package_asset,record=record,
            version_lock=record['versions'][0]['version_lock'],outputs=out,request=request_asset,
            job_id=result['job_id'],human_listening='pending')
        print('DONE',rid,'PASS' if analysis['passed'] else 'RETRY',f"{analysis['qualifying_lines']}/{analysis['total_lines']} lines",flush=True)
        return summary

    def run(self, campaign_path, runtime_asset_path, backend, adopt_backend_revision=None,repeat_after_failures=None,planner_asset_path=None,generation_route=None,repeat_section_lines=None,phrase_max_words=None,asr_dtype=None,timing_reference_slot=None,focus_missing_lines=None,ctc_runtime_asset_path=None,audio_guide_slot=None,audio_guide_strength=None,audio_guide_target_slots=None,takes_per_slot=None,audio_guide_noise_strength=None,brief_asset_path=None):
        if takes_per_slot is not None and (type(takes_per_slot) is not int or not 1<=takes_per_slot<=10):
            raise ValueError('Takes per slot must be between 1 and 10')
        campaign=read(campaign_path)
        if campaign['schema']!='reel.song-audition-campaign.v1':raise ValueError('Unsupported campaign')
        ids=[slot['id'] for slot in campaign['slots']]
        if not ids or len(ids)>64 or len(set(ids))!=len(ids) or any(not re.fullmatch(r'[a-z0-9][a-z0-9-]{0,119}',name) for name in ids):
            raise ValueError('Invalid campaign slots')
        campaign_asset=self.ingest(campaign_path,'Owner-authorized frozen musical audition campaign')
        runtime_asset=read(runtime_asset_path)
        self.locate(runtime_asset)
        backend_asset=self.ingest(backend,'REEL generic offline song backend source')
        state_path=self.work/'state.json'
        if state_path.exists():
            state=read(state_path)
            if state['campaign']['sha256']!=campaign_asset['sha256'] or state['runtime']['sha256']!=runtime_asset['sha256']:
                raise ValueError('Frozen campaign/runtime changed; use an explicit new campaign')
            if state['backend']['sha256']!=backend_asset['sha256']:
                if not adopt_backend_revision or len(adopt_backend_revision.strip())<8:
                    raise ValueError('Backend changed; explicit --adopt-backend-revision reason required')
                state.setdefault('backend_revisions',[]).append(dict(previous=state['backend'],replacement=backend_asset,
                    reason=adopt_backend_revision,qualified_slots_preserved=sorted(state['qualified'])))
                state['backend']=backend_asset
        else:state=dict(campaign=campaign_asset,runtime=runtime_asset,backend=backend_asset,attempts=[],qualified={})
        if audio_guide_slot:
            if audio_guide_slot not in state['qualified']:raise ValueError('Audio guide must already qualify')
            donor=state['qualified'][audio_guide_slot];req=read(self.locate(donor['request']))
            edition=None
            if req['task']=='short-edit':
                edition=req
                req=read(self.locate(edition['source_request']))
            if req['task'] not in ('text2music','cover'):raise ValueError('Only generated takes or verified short editions may supply this guide')
            proposed=dict(slot_id=audio_guide_slot,label=next(slot['label'] for slot in campaign['slots'] if slot['id']==audio_guide_slot),
                recording=donor['outputs']['recording'],request=donor['request'],generation=edition['source_generation'] if edition else donor['outputs']['generation'],
                version_lock=donor['version_lock'],strength=.75 if audio_guide_strength is None else audio_guide_strength)
            if edition:
                proposed.update(request=edition['source_request'],generation=edition['source_generation'],
                    label='short edition '+proposed['label'],short_edit=dict(recipe=donor['request'],receipt=donor['outputs']['edit-receipt']))
            if audio_guide_target_slots is not None:
                proposed['target_slot_ids']=guide_targets(campaign['slots'],req,audio_guide_target_slots)
            if audio_guide_noise_strength is not None:
                if type(audio_guide_noise_strength) not in (int,float) or not 0<=audio_guide_noise_strength<=1:
                    raise ValueError('Guide source-noise strength must be between 0 and 1')
                proposed['noise_strength']=audio_guide_noise_strength
            if state.get('audio_guide')!=proposed:
                state.setdefault('audio_guide_revisions',[]).append(dict(previous=state.get('audio_guide'),replacement=proposed,
                    after_retained_takes=len(state['attempts']),qualified_slots_preserved=sorted(state['qualified'])))
                state['audio_guide']=proposed
        elif audio_guide_strength is not None or audio_guide_target_slots is not None or audio_guide_noise_strength is not None:
            raise ValueError('Guide strength/targets require an explicit guide slot')
        self.audio_guide=state.get('audio_guide')
        brief_changed=False
        if brief_asset_path:
            proposed=read(brief_asset_path)
            if state.get('brief_asset')!=proposed:
                brief_changed=True
                state.setdefault('brief_revisions',[]).append(dict(previous=state.get('brief_asset'),replacement=proposed,after_retained_takes=len(state['attempts']),qualified_slots_preserved=sorted(state['qualified'])))
                state['brief_asset']=proposed
        self.brief_asset=state.get('brief_asset')
        self.briefs={}
        if self.brief_asset:
            lock=self.brief_asset.get('version_lock',{})
            if not all(isinstance(lock.get(key),str) and lock[key] for key in ['logical_id','version_id','manifest_sha256']) or not re.fullmatch(r'[0-9a-f]{64}',lock['manifest_sha256']):
                raise ValueError('Musical brief revision requires a complete immutable version lock')
            briefs=read(self.locate(self.brief_asset))
            if briefs.get('schema')!='reel.song-musical-briefs.v1' or not isinstance(briefs.get('slots'),dict) or set(briefs['slots'])-set(ids):
                raise ValueError('Musical brief revision must name campaign slots')
            self.briefs=briefs['slots']
            if brief_changed and set(self.briefs)&set(state['qualified']):raise ValueError('Musical brief revision cannot change a qualified candidate')
            for slot in campaign['slots']:
                revised_caption(slot,self.briefs)
        if timing_reference_slot and not state.get('timing_reference'):
            if timing_reference_slot not in state['qualified']:raise ValueError('Timing reference must already qualify')
            donor=state['qualified'][timing_reference_slot];req=read(self.locate(donor['request']))
            state['timing_reference']=dict(slot_id=timing_reference_slot,request=donor['request'],version_lock=donor['version_lock'],
                source=req['source'],source_lock=req['source_lock'],seed=req['seed'],duration_seconds=req['duration_seconds'],
                attempt_starts={slot['id']:1+max([a['attempt'] for a in state['attempts'] if a['slot_id']==slot['id']],default=0) for slot in campaign['slots'] if slot['id'] not in state['qualified']},
                after_retained_takes=len(state['attempts']),audio_conditioning=False)
        self.timing_reference=state.get('timing_reference')
        if self.timing_reference:
            if timing_reference_slot and timing_reference_slot!=self.timing_reference['slot_id']:raise ValueError('Frozen timing reference changed')
            req=read(self.locate(self.timing_reference['request']))
            if any(req[k]!=self.timing_reference[k] for k in ['source','source_lock','seed','duration_seconds']):raise ValueError('Timing reference differs from frozen request')
        self.planner_asset=state.get('planner')
        if ctc_runtime_asset_path:
            proposed=read(ctc_runtime_asset_path)
            for key in ['component','script','backend']:self.locate(proposed[key])
            if state.get('ctc_runtime')!=proposed:
                state.setdefault('ctc_runtime_revisions',[]).append(dict(previous=state.get('ctc_runtime'),replacement=proposed,
                    after_retained_takes=len(state['attempts']),qualified_slots_preserved=sorted(state['qualified'])))
                state['ctc_runtime']=proposed
        self.ctc_runtime=state.get('ctc_runtime')
        if planner_asset_path:
            proposed=read(planner_asset_path)
            self.locate(proposed)
            if self.planner_asset and self.planner_asset!=proposed:raise ValueError('Frozen planner changed')
            self.planner_asset=proposed
            state['planner']=proposed
            state.setdefault('planner_adoption',dict(after_retained_takes=len(state['attempts']),qualified_slots_preserved=sorted(state['qualified']),
                policy='One unchanged full-poem pass with optional installed local 5Hz planner; original line gate unchanged'))
        route=generation_route or state.get('generation_route', 'planned' if self.planner_asset else 'direct')
        if route=='planned' and not self.planner_asset:raise ValueError('Planned route requires frozen planner component')
        if generation_route and state.get('generation_route')!=route:
            state.setdefault('generation_route_revisions',[]).append(dict(previous=state.get('generation_route'),replacement=route,after_retained_takes=len(state['attempts'])))
        state['generation_route']=route
        if route=='direct':self.planner_asset=None
        self.repeat_section_lines=repeat_section_lines if repeat_section_lines is not None else state.get('repeat_section_lines')
        if repeat_section_lines is not None and state.get('repeat_section_lines')!=repeat_section_lines:
            state.setdefault('section_policy_revisions',[]).append(dict(previous=state.get('repeat_section_lines'),replacement=repeat_section_lines,after_retained_takes=len(state['attempts'])))
            state['repeat_section_lines']=repeat_section_lines
        self.phrase_max_words=phrase_max_words or state.get('phrase_max_words')
        if phrase_max_words and state.get('phrase_max_words')!=phrase_max_words:
            state.setdefault('phrase_policy_revisions',[]).append(dict(previous=state.get('phrase_max_words'),replacement=phrase_max_words,after_retained_takes=len(state['attempts'])))
            state['phrase_max_words']=phrase_max_words
        self.asr_dtype=asr_dtype or state.get('asr_dtype')
        if asr_dtype and state.get('asr_dtype')!=asr_dtype:
            state.setdefault('asr_policy_revisions',[]).append(dict(previous=state.get('asr_dtype','float32'),replacement=asr_dtype,after_retained_takes=len(state['attempts'])))
            state['asr_dtype']=asr_dtype
        previous_policy=state.get('retry_policy',{})
        if focus_missing_lines is not None:state['focus_missing_lines']=focus_missing_lines
        self.repeat_after_failures=previous_policy.get('repeat_complete_poem_after_failures') if repeat_after_failures is None else repeat_after_failures
        if repeat_after_failures is not None:
            policy=dict(repeat_complete_poem_after_failures=repeat_after_failures,render_passes=2,duration_factor=1.75,
                authority='Explicit operator option under owner permission for repeated lines; source words and line gate unchanged')
            if previous_policy!=policy:
                state.setdefault('retry_policy_revisions',[]).append(dict(previous=previous_policy,replacement=policy))
                state['retry_policy']=policy
        self.save('state.json',state)
        stop_path=self.work/'stop-after-current.json'
        def stop_requested():return stop_path.exists() and read(stop_path).get('requested') is True
        if stop_requested():
            print('STOP REQUESTED; retained campaign can resume after explicit request clear',flush=True)
            return
        for slot in campaign['slots']:
            completed_in_round=0
            while slot['id'] not in state['qualified']:
                if takes_per_slot is not None and completed_in_round>=takes_per_slot:break
                attempt=1+max([a['attempt'] for a in state['attempts'] if a['slot_id']==slot['id']],default=0)
                self.lyric_focus=None
                prior=[a for a in state['attempts'] if a['slot_id']==slot['id']]
                if state.get('focus_missing_lines') and prior:
                    best=max(prior,key=lambda a:(a['qualifying_lines'],a['exact_lines']))
                    report=read(self.locate(best['outputs']['lyric-analysis']))
                    source_lines=[line for line in self.locate(slot['source']).read_text(encoding='utf-8').splitlines() if line.strip()]
                    if [row['canonical'] for row in report['lines']]!=source_lines:raise ValueError('Focus report source differs from frozen canonical lines')
                    numbers=[];word_count=0
                    for row in report['lines']:
                        if not row['passes_gate'] and word_count+row['total_words']<=16:
                            numbers.append(row['line']);word_count+=row['total_words']
                    if numbers:self.lyric_focus=dict(line_numbers=numbers,analysis=best['outputs']['lyric-analysis'],version_lock=best['version_lock'],recognition_prompt=False)
                result=self.run_take(slot,attempt,runtime_asset,backend_asset)
                completed_in_round+=1
                state['attempts'].append(result)
                if result['qualified']:state['qualified'][slot['id']]=result
                self.save('state.json',state)
                self.asset_json('checkpoint.json',state,'Retained audition campaign checkpoint after '+result['id'])
                if result['qualified']:self.review(campaign,state)
                self.save('active.json',dict(stage='line-gate-complete',candidate_id=result['id'],qualified=len(state['qualified']),target=len(ids)))
                if stop_requested():
                    print('DRAINED after completed recording and analysis; all results retained',flush=True)
                    return
        complete=len(state['qualified'])==len(ids)
        self.save('active.json',dict(stage='complete' if complete else 'round-complete',qualified=len(state['qualified']),target=len(ids)))
        self.review(campaign,state)
        print('COMPLETE' if complete else 'ROUND COMPLETE',len(state['qualified']),'qualified slots of',len(ids),len(state['attempts']),'retained takes',flush=True)

    def review(self,campaign,state):
        def media_view(asset,name,extension):
            source=self.locate(asset)
            if source.parent.parent.name!='sha256' or source.parent.parent.parent.name!='objects':
                raise ValueError('Expected canonical cache object path')
            view=source.parents[3]/'views'/'song-auditions'/state['campaign']['sha256'][:16]/(name+'.'+extension)
            view.parent.mkdir(parents=True,exist_ok=True)
            if view.exists():checked(view,asset)
            else:os.link(source,view)  # named disposable same-volume view; no media copy
            return view.as_uri()
        sections=[]
        for slot in campaign['slots']:
            if slot['id'] not in state['qualified']:
                sections.append('<section><h2>'+html.escape(slot['title']+' — '+slot['label'])+'</h2><p>Recording and lyric qualification pending.</p></section>')
                continue
            result=state['qualified'][slot['id']]
            audio=media_view(result['outputs']['listening'],slot['id'],'mp3')
            raw=media_view(result['outputs']['recording'],slot['id'],'wav')
            analysis=read(self.locate(result['outputs']['lyric-analysis']))
            request=read(self.locate(result['request']))
            guide_note='<p>Audio-guided variation from the generated '+html.escape(request['guide_label'])+' recording.</p>' if request['task']=='cover' else ''
            edited=request['task']=='short-edit'
            if edited:guide_note='<p>Short edit of '+html.escape(request['source_label'])+'. Original voice, tempo and pitch retained; repeated sections and excess instrumental time removed.</p>'
            def timing(row):
                span=row.get('recognized_span')
                return '—' if not span or any(t is None for t in span) else '–'.join(f'{int(t)//60}:{int(t)%60:02d}' for t in span)
            rows='<tr><th>Line</th><th>Original Spanish</th><th>Recognition</th><th>Time</th></tr>'+''.join('<tr><td>'+str(row['line'])+'</td><td>'+html.escape(row['canonical'])+'</td><td>'+html.escape(', '.join(row['missing_words']) or 'All words recognized')+'</td><td>'+timing(row)+'</td></tr>' for row in analysis['lines'])
            duration_note=('Edited duration: '+str(request['duration_seconds'])+' seconds. Original generation request: '+str(request['source_generation_duration_seconds'])+' seconds.' if edited else 'Requested duration: '+str(request['duration_seconds'])+' seconds.')
            prompt='<details><summary>'+('Original generation prompt' if edited else 'Submitted music prompt')+'</summary><p>'+html.escape(request['caption'])+'</p><p>'+duration_note+' Seed: '+str(request['seed'])+'.</p></details>'
            ordered=analysis.get('continuous_order_qualifying_lines',analysis['qualifying_lines'])
            order_note='<p>Continuous source-order screen: '+str(ordered)+'/'+str(analysis['total_lines'])+' lines. '+('Listen to the ordering; coverage can use different repeated occurrences.' if ordered<analysis['total_lines'] else 'All lines also qualify in source order.')+'</p>'
            evidence_note='<p>Line evidence derives from original independent recognition; every evidence-bearing audio sample was verified unchanged by this edit. This is not a new transcription.</p>' if edited else ''
            take_note='Short edition.' if edited else 'Take '+str(result['attempt'])+'.'
            sections.append('<section><h2>'+html.escape(slot['title']+' — '+slot['label'])+'</h2><p>'+html.escape(result.get('style',slot['style']))+'</p>'+guide_note+'<audio controls preload="none" src="'+html.escape(audio,quote=True)+'"></audio><p>'+str(result['qualifying_lines'])+'/'+str(result['total_lines'])+' source lines qualify; '+str(result['one_word_gap_lines'])+' with one unrecognized word. '+take_note+'</p>'+order_note+'<p><a href="'+html.escape(raw,quote=True)+'">'+('Edited WAV' if edited else 'Original WAV')+'</a></p>'+prompt+'<details><summary>Line analysis</summary>'+evidence_note+'<table>'+rows+'</table></details></section>')
        page='<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Song auditions A / B / C</title><style>body{font:18px/1.5 system-ui;max-width:960px;margin:2rem auto;padding:1rem;background:#faf7ed;color:#243b32}section{background:white;padding:1.5rem;margin:1.5rem 0;border:1px solid #ccc;border-radius:12px}audio{width:100%}td{padding:.4rem;border-bottom:1px solid #ddd}</style><h1>Song auditions A / B / C</h1><p>Private listening. Every source line has at most one unrecognized word and at least one recognized word in its best diagnostic pass. Repeated lines are allowed. Recognition is evidence; listening and Golden selection remain yours.</p>'+''.join(sections)+'</html>'
        path=self.work/'review.html';path.write_text(page,encoding='utf-8',newline='\n')
        asset=self.ingest(path,'Private static A/B/C audition listening page')
        self.save('review-asset.json',asset)

    def restore(self,state_path,lfs_executable,all_attempts=False):
        """Actual local Git LFS restoration with invocation-only configuration.

        Never retarget repository-wide transfer settings or recover an archive.
        fetch-existing uses an existing active tier and verifies the result.
        """
        state=read(state_path)
        env=os.environ.copy()
        count=int(env.get('GIT_CONFIG_COUNT','0'))
        settings=[('lfs.customtransfer.caimitos-cache.path',str(Path(lfs_executable).resolve())),
            ('lfs.customtransfer.caimitos-cache.args','--root "'+str(self.root).replace('\\','/')+'"')]
        for i,(key,value) in enumerate(settings,count):
            env[f'GIT_CONFIG_KEY_{i}']=key;env[f'GIT_CONFIG_VALUE_{i}']=value
        env['GIT_CONFIG_COUNT']=str(count+len(settings))
        results={}
        items=[(row['id'],row) for row in state['attempts']] if all_attempts else list(state['qualified'].items())
        for name,row in items:
            results[name]={}
            for role in ['recording','listening','lyric-analysis']:
                results[name][role]=self.call('fetch-existing',row['outputs'][role]['sha256'],env=env)
            print('RESTORED',name,'raw, preview and line analysis through local Git LFS',flush=True)
        asset=self.asset_json('restoration.json',results,'Actual local Git LFS restoration of qualified song auditions')
        self.save('restoration-asset.json',asset)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root',required=True);p.add_argument('--assets',required=True);p.add_argument('--work',required=True)
    p.add_argument('--metadata-retries',type=int,choices=range(0,31),default=0,
        help='Explicit bounded retry count for a busy metadata lock; all other failures stop')
    sub=p.add_subparsers(dest='operation',required=True)
    snapshot=sub.add_parser('snapshot')
    for name in ['ace-root','whisper-root','demucs','python','ffmpeg']:snapshot.add_argument('--'+name,required=True)
    run=sub.add_parser('run');run.add_argument('--campaign',required=True);run.add_argument('--runtime-asset',required=True)
    run.add_argument('--backend',default=str(Path(__file__).with_name('local_song_backend.py')))
    run.add_argument('--adopt-backend-revision',help='Explicit reason to adopt a new adapter hash; past requests/evidence and qualified slots stay immutable')
    run.add_argument('--repeat-after-failures',type=int,choices=range(1,31),help='Explicitly authorize two unchanged full-poem passes after N failed takes; preserves the line gate')
    run.add_argument('--planner-asset',help='Explicitly adopt a frozen local planner component for future takes; one unchanged source pass')
    run.add_argument('--generation-route',choices=['direct','planned'],help='Explicit route revision for future takes; planner component history stays retained')
    run.add_argument('--repeat-section-lines',type=int,choices=[0,*range(2,9)],help='Supply each unchanged source section twice; 0 explicitly disables section repetition')
    run.add_argument('--phrase-max-words',type=int,choices=range(4,17),help='Join short source fragments into bounded sung phrases without crossing original stanzas or changing words')
    run.add_argument('--asr-dtype',choices=['float32','float16'],help='Explicit GPU recognition precision; model, three passes and line gate stay fixed')
    run.add_argument('--timing-reference-slot',help='Use a qualified same-source request duration and seed neighborhood for future distinct musical briefs; no audio conditioning')
    run.add_argument('--focus-missing-lines',action=argparse.BooleanOptionalAction,default=None,help='Remind generation of bounded exact source phrases missed by the best prior take; ASR stays unprompted')
    run.add_argument('--ctc-runtime-asset',help='Explicit frozen independent Spanish CTC component for bounded near-pass rechecks; no downloads or lyric prompt')
    run.add_argument('--audio-guide-slot',help='Explicit generated same-poem qualified text2music recording as a cover guide; private scope and receipt verified')
    run.add_argument('--audio-guide-strength',type=float,help='Explicit cover influence from 0.2 to 1.0; default 0.75 with a guide')
    run.add_argument('--audio-guide-target-slot',action='append',help='Restrict a generated guide to explicitly named same-source slots; repeat for multiple targets')
    run.add_argument('--takes-per-slot',type=int,choices=range(1,11),help='Complete a bounded serial round before returning; pending slots remain pending and qualified slots stay fixed')
    run.add_argument('--audio-guide-noise-strength',type=float,help='Explicit installed ACE source initialization: 0 starts from noise, 1 stays closest to the generated guide; separate from conditioning strength')
    run.add_argument('--brief-asset',help='Explicit frozen, source-scoped musical caption revision for pending slots; prior requests and qualified takes stay immutable')
    restore=sub.add_parser('restore');restore.add_argument('--state',required=True);restore.add_argument('--lfs-executable',required=True)
    restore.add_argument('--all-attempts',action='store_true',help='Verify retained failed takes as well as qualifying candidates')
    review=sub.add_parser('review');review.add_argument('--campaign',required=True);review.add_argument('--state',required=True)
    stop=sub.add_parser('request-stop');stop.add_argument('--clear',action='store_true')
    args=p.parse_args();c=Campaign(args.root,args.assets,args.work,args.metadata_retries)
    if args.operation=='snapshot':c.freeze_runtime(args)
    elif args.operation=='run':c.run(args.campaign,args.runtime_asset,args.backend,args.adopt_backend_revision,args.repeat_after_failures,args.planner_asset,args.generation_route,args.repeat_section_lines,args.phrase_max_words,args.asr_dtype,args.timing_reference_slot,args.focus_missing_lines,args.ctc_runtime_asset,args.audio_guide_slot,args.audio_guide_strength,args.audio_guide_target_slot,args.takes_per_slot,args.audio_guide_noise_strength,args.brief_asset)
    elif args.operation=='restore':c.restore(args.state,args.lfs_executable,args.all_attempts)
    elif args.operation=='review':c.review(read(args.campaign),read(args.state))
    else:c.save('stop-after-current.json',dict(requested=not args.clear))


if __name__=='__main__':main()
