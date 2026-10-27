import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
const source=readFileSync(new URL('../ui/local-overlay.js',import.meta.url),'utf8');
function overlay(width=1000){
 const events=new Map(),calls=[]; let closed=0;
 const style={setProperty(k,v){this[k]=v;}};
 const panel={style,classList:{add(){}},focus(){doc.activeElement=this;},querySelectorAll(){return controls;}};
 const controls=[0,1].map(()=>({hidden:false,getClientRects:()=>[{}],focus(){doc.activeElement=this;}}));
 const closer={click(){closed++;}};
 const doc={body:{},documentElement:{},activeElement:null,querySelector:s=>s==='#local-panel'?panel:s==='#close'?closer:{},addEventListener:(n,fn)=>events.set(n,fn)};
 const win={innerWidth:width,addEventListener:(n,fn)=>events.set(n,fn),__TAURI__:{core:{invoke:async c=>calls.push(c)}}};
 runInNewContext(source,{window:win,document:doc,console,requestAnimationFrame:fn=>fn()});
 return {events,calls,win,doc,panel,controls,get closed(){return closed;}};
}
test('Local overlay anchors panel and clamps edges on small windows',()=>{
 const a=overlay(420); a.win.__anivoxOverlayAnchor(1);
 assert.equal(a.panel.style.left,'8px'); assert.equal(a.panel.style.width,'404px');
 assert.equal(a.calls[0],'local_overlay_ready');
 a.win.innerWidth=1280; a.win.__anivoxOverlayAnchor(0);
 assert.equal(a.panel.style.left,'8px');
 a.win.__anivoxOverlayAnchor(NaN); assert.equal(a.panel.style.left,'8px');
});
test('Outside click uses existing close button; inside does not close',()=>{
 const a=overlay(); a.events.get('click')({target:a.panel}); assert.equal(a.closed,0);
 a.events.get('click')({target:a.doc.body}); assert.equal(a.closed,1);
});
test('Tab focus stays inside dialog in both directions',()=>{
 const a=overlay(); let prevented=0;
 a.doc.activeElement=a.controls[1]; a.events.get('keydown')({key:'Tab',preventDefault(){prevented++;}});
 assert.equal(a.doc.activeElement,a.controls[0]);
 a.events.get('keydown')({key:'Tab',shiftKey:true,preventDefault(){prevented++;}});
 assert.equal(a.doc.activeElement,a.controls[1]); assert.equal(prevented,2);
});
test('Overlay documents remain local and accessible, no iframes',()=>{
 for(const name of ['settings','update-review']){
  const html=readFileSync(new URL(`../ui/${name}.html`,import.meta.url),'utf8');
  assert.match(html,/role="dialog"/); assert.match(html,/aria-modal="true"/);
  assert.match(html,/local-overlay\.js/); assert.doesNotMatch(html,/<iframe/i);
 }
});
