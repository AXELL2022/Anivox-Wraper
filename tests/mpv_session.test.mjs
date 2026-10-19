import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
const file=new URL('../src-tauri/scripts/mpv-session.js',import.meta.url);
const source=existsSync(file)?readFileSync(file,'utf8'):'';
function setup(){
 const window={}; runInNewContext(source,{window,console,Number,Math});
 assert.equal(typeof window.__anivoxCreateMpvSession,'function');
 let identity='1:2:3',generation=1; const qualities=[];
 const video={muted:false,volume:.7,currentTime:25,paused:false,playbackRate:1,pause(){this.paused=true},async play(){this.paused=false}};
 const context={quality:'1080',GetQualities:[{id:'1080'},{id:'360'},{id:'720'},{id:'144_upscale'},{id:'144',disabled:true}],async changeQuality(q){qualities.push(q.id);this.quality=q.id}};
 const player=window.__anivoxCreateMpvSession({getVideo:()=>video,getContext:()=>context,getIdentity:()=>identity,getGeneration:()=>generation});
 const snapshot=(overrides={})=>({sessionId:'test',mediaKey:'1:2:3',ready:true,mirrorEmbedded:true,paused:false,buffering:false,position:25,duration:1200,speed:1,...overrides});
 return {player,video,context,qualities,snapshot,navigate(){identity='1:3:3';generation++}};
}
test('Mirror starts only after MPV file-loaded and selects lowest playable quality',async()=>{
 const a=setup();await a.player.begin({sessionId:'test',mirrorEmbedded:true},'1:2:3');
 await a.player.update(a.snapshot({ready:false}));assert.equal(a.video.muted,false);assert.deepEqual(a.qualities,[]);
 await a.player.update(a.snapshot());assert.equal(a.video.muted,true);assert.equal(a.video.paused,false);assert.deepEqual(a.qualities,['360']);
 await a.player.update(a.snapshot());assert.deepEqual(a.qualities,['360']);
});
test('Mirror follows pause, buffering, seek and speed, then restores quality and sound',async()=>{
 const a=setup();await a.player.begin({sessionId:'test',mirrorEmbedded:true},'1:2:3');
 await a.player.update(a.snapshot({position:90,speed:1.5,paused:true}));assert.equal(a.video.currentTime,90);assert.equal(a.video.paused,true);assert.equal(a.video.playbackRate,1.5);
 await a.player.update(a.snapshot({position:95,buffering:true}));assert.equal(a.video.paused,true);
 await a.player.update(a.snapshot({position:100}));assert.equal(a.video.paused,false);
 await a.player.update(null);assert.equal(a.video.muted,false);assert.equal(a.video.volume,.7);assert.equal(a.video.paused,true);assert.equal(a.context.quality,'1080');assert.equal(a.video.playbackRate,1);
});
test('Stale MPV session cannot change replacement playback',async()=>{
 const a=setup();await a.player.begin({sessionId:'test',mirrorEmbedded:true},'1:2:3');
 await a.player.update(a.snapshot());a.navigate();await a.player.update(a.snapshot({position:999}));assert.equal(a.video.currentTime,25);assert.equal(a.video.muted,false);
});
test('Mirror disabled pauses embedded only after MPV is ready',async()=>{
 const a=setup();await a.player.begin({sessionId:'test',mirrorEmbedded:false},'1:2:3');
 await a.player.update(a.snapshot({ready:false}));assert.equal(a.video.paused,false);
 await a.player.update(a.snapshot());assert.equal(a.video.paused,true);assert.equal(a.video.muted,false);assert.deepEqual(a.qualities,[]);
});

test('Async quality restoration cannot overwrite a newly selected episode',async()=>{
 const a=setup();await a.player.begin({sessionId:'test',mirrorEmbedded:true},'1:2:3');await a.player.update(a.snapshot());
 let finish; const delayed=new Promise(resolve=>finish=resolve);
 a.context.changeQuality=()=>delayed;
 const stopping=a.player.stop();
 await Promise.resolve();a.navigate();
 a.video.paused=false;a.video.playbackRate=1.5;a.video.muted=true;a.video.volume=.2;
 finish();await stopping;
 assert.equal(a.video.paused,false);assert.equal(a.video.playbackRate,1.5);assert.equal(a.video.muted,true);assert.equal(a.video.volume,.2);
});

test('Late quality change from previous MPV session cannot stop its replacement',async()=>{
 const a=setup();let finish;let first=true;
 const delayed=new Promise(resolve=>finish=resolve);
 a.context.changeQuality=async q=>{a.context.quality=q.id;if(q.id==='360'&&first){first=false;await delayed}};
 await a.player.begin({sessionId:'test',mirrorEmbedded:true},'1:2:3');
 const previous=a.player.update(a.snapshot());await Promise.resolve();
 await a.player.begin({sessionId:'replacement',mirrorEmbedded:true},'1:2:3');
 finish();await previous;
 await a.player.update(a.snapshot({sessionId:'replacement'}));
 assert.equal(a.video.muted,true);assert.equal(a.video.paused,false);
});
