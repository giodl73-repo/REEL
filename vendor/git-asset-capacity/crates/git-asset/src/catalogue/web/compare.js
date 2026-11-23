'use strict';
const recordComparison={baseline:null,generation:0};
function comparisonPin(asset){return {project:asset.project,id:asset.id,title:asset.title,commit:asset.details.commit,record:JSON.parse(JSON.stringify(asset.details.record))};}
function sameComparisonRecord(a,b){return a?.id===b?.id&&a?.object?.sha256===b?.object?.sha256&&a?.object?.bytes===b?.object?.bytes;}
function comparisonCurrent(asset,view,sequence){return state.selected===asset&&state.tab==='Compare'&&asset.comparisonView===view&&view.sequence===sequence&&view.baselineGeneration===recordComparison.generation&&view.commit===asset.details?.commit;}
function chooseComparisonBaseline(asset,clear=false){
 if(state.selected!==asset||state.tab!=='Compare')return;
 recordComparison.baseline=clear?null:comparisonPin(asset);recordComparison.generation++;drawDetail();document.getElementById(clear?'compare-choose':'compare-run')?.focus();
}
async function compareWithBaseline(asset,view){
 const baseline=recordComparison.baseline;if(!baseline)return;
 const current=comparisonPin(asset),sequence=++view.sequence;view.loading=true;view.error='';view.result=null;drawDetail();
 try{
  const report=await api('/api/compare',{left_project:baseline.project,left_id:baseline.id,left_snapshot:baseline.commit,right_project:current.project,right_id:current.id,right_snapshot:current.commit});
  if(!comparisonCurrent(asset,view,sequence))return;
  const matches=(side,pin)=>side?.selector?.project===pin.project&&side?.selector?.id===pin.id&&sameComparisonRecord(side.record,pin.record);
  if(report.snapshots?.[baseline.project]!==baseline.commit||report.snapshots?.[current.project]!==current.commit||!matches(report.data?.left,baseline)||!matches(report.data?.right,current))throw Error('Comparison source changed. Refresh the affected project and choose the baseline again.');
  view.result=report;
 }catch(e){if(comparisonCurrent(asset,view,sequence)){view.error=e.message==='stale_basis'||e.message.includes('Snapshot changed')?'Refresh the affected project and choose the baseline again. ('+e.message+')':e.message;view.result=null;}}
 finally{if(comparisonCurrent(asset,view,sequence)){view.loading=false;drawDetail();document.getElementById('comparison-feedback')?.focus();}else if(view.sequence===sequence)view.loading=false;}
}
function comparisonIdentity(label,pin){
 const s=section(label);field(s,'Project',projectLabel(pin.project));field(s,'Asset',pin.title+' · '+pin.id);field(s,'Snapshot',pin.commit);field(s,'Record SHA-256',pin.record.object.sha256);return s;
}
function comparisonBinding(label,binding){
 const s=node('div','stream');s.append(node('strong','',label+' · '+binding.slot),node('p','',binding.role+' · '+binding.group+' · '+bytes(binding.object.bytes)),node('div','mono',binding.object.sha256));return s;
}
function drawComparison(asset,area){
 let view=asset.comparisonView;
 if(!view||view.commit!==asset.details.commit||view.baselineGeneration!==recordComparison.generation)view=asset.comparisonView={commit:asset.details.commit,baselineGeneration:recordComparison.generation,sequence:0,loading:false,result:null,error:''};
 const baseline=recordComparison.baseline,current=comparisonPin(asset);
 area.append(node('p','muted','Choose a baseline, then open another record and compare. The baseline keeps its recorded snapshot while you navigate between projects.'));
 if(baseline)area.append(comparisonIdentity('Baseline record',baseline));else area.append(node('p','callout','No baseline selected. Use this record as a baseline, then open the other asset’s Compare tab.'));
 area.append(comparisonIdentity('Current record',current));
 const choose=node('button','button secondary','Use this record as baseline');choose.id='compare-choose';choose.onclick=()=>chooseComparisonBaseline(asset);
 const clear=node('button','button secondary','Clear baseline');clear.disabled=!baseline;clear.onclick=()=>chooseComparisonBaseline(asset,true);
 const run=node('button','button',view.loading?'Comparing…':'Compare with baseline');run.id='compare-run';run.disabled=!baseline||view.loading;run.onclick=()=>compareWithBaseline(asset,view);area.append(choose,clear,run);
 const feedback=node('div');feedback.id='comparison-feedback';feedback.tabIndex=-1;feedback.setAttribute('aria-live','polite');area.append(feedback);
 if(view.error){feedback.append(node('p','callout',view.error));return;}
 if(view.loading){feedback.append(node('p','muted','Comparing registered metadata in the exact snapshots…'));return;}
 if(!view.result)return;
 const data=view.result.data,label=value=>value===true?'Same':value===false?'Different':'Unavailable';
 const summary=section('Comparison result');field(summary,'Selected bytes',data.selected_equal==null?'Unavailable · one or both records have no selected bytes':label(data.selected_equal)+' registered SHA-256 and length');field(summary,'Record bytes',label(data.record_equal));field(summary,'Definition',label(data.definition_equal));feedback.append(summary);
 const authored=section('Changed authored field names');data.authored_changed_fields.forEach(name=>authored.append(node('p','mono',name)));if(!data.authored_changed_fields.length)authored.append(node('p','muted','No authored field differences.'));authored.append(node('p','muted','Values are omitted; field names do not establish whether regeneration is needed.'));feedback.append(authored);
 const streams=section('Named streams · baseline → current');streams.append(node('p','',data.streams.added.length+' added · '+data.streams.removed.length+' removed · '+data.streams.changed.length+' changed · '+data.streams.unchanged+' unchanged'));
 for(const kind of ['added','removed'])for(const binding of data.streams[kind])streams.append(comparisonBinding(kind==='added'?'Added':'Removed',binding));
 for(const change of data.streams.changed){const details=node('details');details.append(node('summary','','Changed slot · '+change.slot),comparisonBinding('Before',change.before),comparisonBinding('After',change.after));streams.append(details);}feedback.append(streams);
 feedback.append(node('p','callout','Equal selected bytes can belong to different records or recipes. This comparison checks registered identities and bindings; payload availability, decoded media and transitive dependencies are separate checks. No cache eviction is authorized.'));
}
