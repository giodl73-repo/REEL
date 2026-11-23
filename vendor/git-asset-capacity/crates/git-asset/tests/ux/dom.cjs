'use strict';
// Deliberately small controller DOM double: no layout or browser claims.
function documentFixture(html){
 const ids=new Map(),listeners=new Map(),document={activeElement:null};
 class Element {
  constructor(tag){this.tag=tag;this.children=[];this.attributes={};this.dataset={};this.hidden=false;this.disabled=false;this.value='';this.className='';this._text='';}
  set id(value){this._id=value;ids.set(value,this);}get id(){return this._id;}
  set textContent(value){this._text=String(value??'');this.children=[];}get textContent(){return this._text+this.children.map(e=>e.textContent).join('');}
  get childElementCount(){return this.children.length;}
  get classList(){return {contains:c=>this.className.split(/\s+/).includes(c),add:c=>{if(!this.classList.contains(c))this.className+=' '+c;},remove:c=>{this.className=this.className.split(/\s+/).filter(v=>v!==c).join(' ');}};}
  append(...items){for(const child of items){if(child.parent)child.parent.children=child.parent.children.filter(c=>c!==child);child.parent=this;this.children.push(child);}}
  replaceChildren(...items){for(const child of this.children)child.parent=null;this.children=[];this._text='';this.append(...items);}
  setAttribute(k,v){this.attributes[k]=String(v);}getAttribute(k){return this.attributes[k]??null;}
  addEventListener(k,fn){(this.listeners??={})[k]=fn;}
  focus(){document.activeElement=this;}
  contains(target){return this===target||this.children.some(c=>c.contains(target));}
  querySelectorAll(selector){return walk(this).filter(e=>matches(e,selector));}
  set innerHTML(_){throw Error('Fixture refuses HTML insertion');}
 }
 const walk=root=>[root,...root.children.flatMap(walk)];
 const matches=(e,selector)=>selector.split(',').some(s=>{s=s.trim();if(s.startsWith('.'))return e.classList.contains(s.slice(1));if(s.startsWith('#'))return e.id===s.slice(1);return e.tag===s;});
 const body=new Element('body');
 for(const match of html.matchAll(/<([a-z][a-z0-9-]*)\b([^>]*\bid="([^"]+)"[^>]*)>/g)){
  const e=new Element(match[1]);e.id=match[3];e.hidden=/\bhidden\b/.test(match[2]);body.append(e);
 }
 const get=id=>{const e=ids.get(id);if(!e)throw Error('Unknown fixture DOM ID: '+id);return e;};
 const inspector=get('inspector');['back-work','close','asset-title','asset-id','asset-badges','ancestry','tabs','detail'].forEach(id=>inspector.append(get(id)));
 const stats=new Element('section'),toolbar=new Element('div'),footer=new Element('footer');stats.className='stats';toolbar.className='toolbar';
 ['search','search-scope','search-kind','snapshot','refresh'].forEach(id=>toolbar.append(get(id)));body.append(stats,toolbar,footer);
 get('search-scope').value='project';
 document.createElement=tag=>new Element(tag);document.getElementById=id=>ids.get(id)||null;
 document.querySelector=selector=>selector==='meta[name="git-asset-session"]'?{content:'synthetic-session'}:selector==='main footer'?footer:walk(body).find(e=>matches(e,selector))||null;
 document.querySelectorAll=selector=>walk(body).filter(e=>matches(e,selector));
 document.addEventListener=(kind,fn)=>listeners.set(kind,fn);
 document.dispatchEvent=event=>listeners.get(event.type)?.(event);
 const visible=e=>!e.hidden&&(!e.parent||visible(e.parent));
 async function click(e,event={}){
  if(!visible(e)||e.disabled)throw Error('Fixture cannot click a hidden/disabled control');
  if(!inspector.hidden&&!inspector.contains(e))throw Error('Inspector modal must be closed first');
  const ev={target:e,currentTarget:e,preventDefault(){},...event};
  if(e.type==='checkbox'){e.checked=!e.checked;return e.onchange?.(ev);}
  return e.onclick?.(ev);
 }
 return {document,get,walk,body,visible,click,Option:function(text,value){const e=new Element('option');e.textContent=text;e.value=value;return e;}};
}
module.exports={documentFixture};
