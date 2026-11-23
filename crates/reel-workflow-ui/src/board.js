"use strict";
(() => {
  const $ = id => document.getElementById(id);
  const state = {data:null, language:null, scene:null, unit:null, versions:new Map(), cue:null, epoch:0, revision:0, board:null, poll:null};
  const audio = $("audio");
  const node = (tag, cls, text) => {const n=document.createElement(tag);if(cls)n.className=cls;if(text!==undefined)n.textContent=text;return n;};
  const clock = seconds => `${String(Math.floor(seconds/60)).padStart(2,"0")}:${(seconds%60).toFixed(2).padStart(5,"0")}`;
  const versions = unit => unit.versions.filter(v=>!v.language || v.language===state.language);
  function chosen(unit) {
    const visible = versions(unit);
    return visible.find(version => version.id === state.versions.get(unit.id))
      ?? visible.findLast(version => version.designation === "selected")
      ?? visible.at(-1);
  }
  const activeCue = () => state.data?.cues.find(c=>c.id===state.cue && c.language===state.language);
  const designation = v => v.designation==="candidate"?"Unselected candidate":v.designation==="selected"?"Selected reference":"Historical reference";
  function image(version, cls) {
    const holder=node("div",cls||"preview-button");
    const empty=node("div","preview-empty");empty.append(node("b","",version?"◇":"＋"),node("span","",version?"Preview unavailable":"Awaiting artwork"));holder.append(empty);
    if(version?.preview){const img=node("img");img.loading="lazy";img.alt=`${version.label} preview`;img.src=version.preview;img.addEventListener("load",()=>empty.hidden=true);img.addEventListener("error",()=>{img.hidden=true;empty.hidden=false;});holder.append(img);}
    return holder;
  }
  function versionButtons(unit) {
    const strip=node("div","version-strip");
    for(const version of versions(unit)){const button=node("button",`version-chip${chosen(unit)?.id===version.id?" active":""}`,version.label);button.setAttribute("aria-label",`${unit.display_id}: ${version.label}, ${designation(version)}`);button.addEventListener("click",()=>{state.versions.set(unit.id,version.id);state.unit=unit.id;renderGrid();renderInspector();});strip.append(button);}
    return strip;
  }
  function selectUnit(unit) {
    state.unit=unit.id;renderGrid();renderInspector();
    const span=unit.spans.find(s=>s.language===state.language);
    if(span){setCue(span.cue_id);const seconds=span.start_sample/activeCue().sample_rate;if(audio.readyState>0)audio.currentTime=seconds;else audio.addEventListener("loadedmetadata",()=>audio.currentTime=seconds,{once:true});}
  }
  function renderGrid() {
    const container=$("storyboard");container.replaceChildren();
    for(const scene of state.data.scenes){if(state.scene && state.scene!==scene.id)continue;
      const units=state.data.units.filter(u=>u.scene_id===scene.id && (!u.spans.length || u.spans.some(s=>s.language===state.language)));
      if(!units.length)continue;
      const heading=node("div","scene-heading");heading.append(node("h2","",scene.title),node("small","",`${units.length} ${state.data.format==="comic"?"pages":"cels"}`));container.append(heading);
      const grid=node("div","grid");
      for(const unit of units){const version=chosen(unit);const card=node("article",`unit-card${state.unit===unit.id?" focused":""}`);card.dataset.unit=unit.id;
        const preview=image(version);preview.setAttribute("role","button");preview.tabIndex=0;preview.setAttribute("aria-label",`Inspect ${unit.display_id}: ${unit.title}`);preview.addEventListener("click",()=>selectUnit(unit));preview.addEventListener("keydown",e=>{if(e.key==="Enter"||e.key===" "){e.preventDefault();selectUnit(unit);}});
        preview.append(node("span",`preview-badge ${version?.designation||"waiting"}`,version?designation(version):"Planned"));card.append(preview);
        const body=node("div","card-body"),title=node("button","card-title");title.append(node("strong","",unit.display_id),node("span","",unit.title));title.addEventListener("click",()=>selectUnit(unit));body.append(title);
        const meta=node("div","card-meta");const task=state.data.tasks[unit.task];meta.append(node("span","",unit.story_id),node("span","",task?.label||`${versions(unit).length} version${versions(unit).length===1?"":"s"}`));body.append(meta);card.append(body,versionButtons(unit));grid.append(card);
      }container.append(grid);
    }highlight();
  }
  function renderInspector() {
    const unit=state.data.units.find(u=>u.id===state.unit),root=$("inspector");root.replaceChildren();if(!unit){root.append(node("p","muted","Choose a cel to inspect its versions and cue."));return;}
    const version=chosen(unit);root.append(node("h2","",unit.display_id),node("p","muted",unit.title));
    const preview=image(version,"preview-button detail-image");root.append(preview);
    if(version)root.append(node("span",`pill ${version.designation}`,designation(version)));
    root.append(versionButtons(unit));
    const span=unit.spans.find(s=>s.language===state.language),cue=state.data.cues.find(c=>c.id===span?.cue_id&&c.language===state.language);
    if(cue){root.append(node("h3","","CUE · "+cue.id),node("p","transcript",cue.text),node("p","muted",`Cel starts at ${clock(span.start_sample/cue.sample_rate)} on this cue’s clock.`));}
    const details=node("details");details.append(node("summary","","Source & lineage"));const list=node("dl");
    for(const [label,value] of [["Story",unit.story_id],["Semantic event",unit.semantic_id],["Asset",version?.asset.logical_id],["SHA-256",version?.asset.sha256],["Approval",version?.approval_reference?"Reference recorded; authority not verified by this viewer.":"No approval reference on this version."]]){if(value){list.append(node("dt","",label),node("dd","",value));}}details.append(list);root.append(details);
  }
  function setCue(id) {
    const cue=state.data.cues.find(c=>c.id===id&&c.language===state.language);
    if(!cue){audio.pause();audio.removeAttribute("src");state.cue=null;$("cue-title").textContent="No cue in this language";$("cue-clock").textContent="";$("markers").replaceChildren();return;}
    const same=state.cue===id && audio.getAttribute("src")===cue.preview;state.cue=id;
    if(!same){audio.pause();if(cue.preview)audio.src=cue.preview;else audio.removeAttribute("src");}
    $("cue-title").textContent=cue.title;$("cue-clock").textContent=`${cue.language.toUpperCase()} · ${cue.sample_rate.toLocaleString()} Hz · ${cue.samples.toLocaleString()} samples`;
    $("audio-status").textContent=cue.preview?"Selected recording · press Play to listen. Source text and clocks remain unchanged.":"No preview declared for this selected recording.";
    const markers=$("markers");markers.replaceChildren();for(const marker of cue.markers){const button=node("button","marker",marker.word);button.dataset.sample=marker.sample;button.title=marker.semantic_id;button.append(node("time","",clock(marker.sample/cue.sample_rate)));button.addEventListener("click",()=>{audio.currentTime=marker.sample/cue.sample_rate;});markers.append(button);}highlight();
  }
  function highlight(){const cue=activeCue();if(!cue)return;const sample=Math.floor((audio.currentTime||0)*cue.sample_rate);$("playhead").textContent=clock(audio.currentTime||0);
    for(const card of document.querySelectorAll(".unit-card")){const unit=state.data.units.find(u=>u.id===card.dataset.unit);card.classList.toggle("is-playing",!audio.paused&&unit.spans.some(s=>s.language===state.language&&s.cue_id===cue.id&&sample>=s.start_sample&&sample<s.end_sample));}
    const active=cue.markers.filter(m=>m.sample<=sample).at(-1);for(const button of $("markers").children)button.classList.toggle("active",Number(button.dataset.sample)===active?.sample);
  }
  function renderCues(){const root=$("cue-sheet");root.replaceChildren();for(const cue of state.data.cues.filter(c=>c.language===state.language)){const row=node("button","cue-row");row.append(node("strong","",cue.title),node("small","",` · ${clock(cue.samples/cue.sample_rate)}`),node("p","",cue.text));row.addEventListener("click",()=>{setCue(cue.id);$("audio").scrollIntoView({block:"center",behavior:"smooth"});});root.append(row);}}
  function renderTasks(){const root=$("tasks");root.replaceChildren();for(const [id,task] of Object.entries(state.data.tasks)){const row=node("div","task");row.append(node("span","task-icon",task.state==="retained-recorded"?"◇":"◷"));const text=node("div");text.append(node("strong","",id),node("small","",task.attempt_id||task.code||"Waiting for a journal observation"));row.append(text,node("span",`pill ${task.state==="waiting"?"waiting":""}`,task.label));root.append(row);}if(!root.children.length)root.append(node("p","muted","No workflow journals are attached to this view."));}
  function render(){const data=state.data;document.title=`${data.title} · REEL`;$("title").textContent=data.title;$("work-id").textContent=data.work_id;$("workspace").replaceChildren(document.createTextNode(data.project),node("small","",data.work_id));$("synthetic").hidden=!data.synthetic;$("source").textContent=`Source ${data.source_commit.slice(0,10)}`;
    $("unit-label").textContent=data.format==="comic"?"PAGES":"CELS";$("story-tab").textContent=data.format==="comic"?"Pages":"Storyboard";$("unit-count").textContent=data.units.length;$("cue-count").textContent=data.cues.filter(c=>c.language===state.language).length;$("candidate-count").textContent=data.units.flatMap(versions).filter(v=>v.designation==="candidate").length;$("waiting-count").textContent=Object.values(data.tasks).filter(t=>["waiting","publishing","updating"].includes(t.state)).length;$("workflow-note").textContent="Tasks awaiting a completed observation";
    const languages=$("language");languages.replaceChildren();for(const language of data.languages){const option=node("option","",language.label);option.value=language.id;option.selected=language.id===state.language;languages.append(option);}
    const scenes=$("scenes");scenes.replaceChildren();for(const scene of [{id:null,title:"All scenes"},...data.scenes]){const button=node("button",state.scene===scene.id?"active":"",scene.title);button.addEventListener("click",()=>{state.scene=scene.id;render();});scenes.append(button);}
    renderGrid();renderInspector();renderCues();renderTasks();setCue(state.cue||data.cues.find(c=>c.language===state.language)?.id);
  }
  async function refresh(force=false){const epoch=++state.epoch;try{const response=await fetch("state.json",{cache:"no-store"});if(!response.ok)throw new Error("Workspace observation unavailable");const data=await response.json();if(epoch!==state.epoch)return;
      if(state.board&&state.board!==data.board_pin)throw new Error("Source changed. Reopen the pinned workspace.");if(data.revision<state.revision) return;const changed=!state.data||state.data.basis_sha256!==data.basis_sha256;state.board=data.board_pin;state.revision=data.revision;state.data=data;state.language ||=data.languages[0].id;state.unit ||=data.units[0]?.id;
      const uncertain=Object.values(data.tasks).some(t=>["updating","needs-recovery","unavailable"].includes(t.state));$("connection").classList.toggle("warn",uncertain);$("connection").textContent=uncertain?"Connected · Some observations need attention. Existing references remain visible.":"● Connected · Read-only observations update automatically";if(changed||force)render();
    }catch(error){if(epoch!==state.epoch)return;$("connection").classList.add("warn");$("connection").textContent="Disconnected · Last view preserved. Reopen the host if its session expired. Jobs continue independently.";}}
  $("language").addEventListener("change",e=>{audio.pause();state.language=e.target.value;state.cue=null;render();});$("refresh").addEventListener("click",()=>refresh(true));
  $("story-tab").addEventListener("click",()=>{ $("storyboard").hidden=false;$("cue-sheet").hidden=true;$("story-tab").setAttribute("aria-selected","true");$("cue-tab").setAttribute("aria-selected","false");});
  $("cue-tab").addEventListener("click",()=>{$("storyboard").hidden=true;$("cue-sheet").hidden=false;$("story-tab").setAttribute("aria-selected","false");$("cue-tab").setAttribute("aria-selected","true");});
  for(const event of ["timeupdate","play","pause","ended"])audio.addEventListener(event,highlight);
  audio.addEventListener("loadedmetadata",()=>{const cue=activeCue();if(cue&&Math.abs(audio.duration-cue.samples/cue.sample_rate)>.1)$("audio-status").textContent="Preview duration differs from the declared take clock. No timing was changed.";});audio.addEventListener("error",()=>$("audio-status").textContent="Recording preview unavailable. The selected take and its clock are preserved.");
  refresh();state.poll=setInterval(()=>refresh(),2000);
})();
