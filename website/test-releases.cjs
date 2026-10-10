// Exercise release lookup and detection without making a network request.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/releases.js`, 'utf8');
async function run(platform, response, userAgent = '', maxTouchPoints = 0) {
  const badge = {hidden:true};
  const download = {dataset:{},querySelector:()=>badge};
  const label = {textContent:'Latest nightly',href:'fallback'};
  const date = {textContent:''};
  let selected;
  const context = {
    navigator:{platform,userAgent,maxTouchPoints}, URL, Date, AbortSignal,
    document:{querySelector:selector=>selector === '#release-version' ? label : selector === '#release-date' ? date : (selected=selector,download)},
    fetch:async()=>response,
  };
  vm.runInNewContext(source, context);
  await new Promise(resolve=>setImmediate(resolve));
  return {badge,download,label,date,selected};
}
(async()=>{
  const good = {ok:true,json:async()=>({tag_name:'v0.3.393-nightly.test',html_url:'https://github.com/DerpcatMusic/KONTRA/releases/tag/v0.3.393',published_at:'2026-10-09T19:00:00Z'})};
  for(const [platform,expected] of [['Win32','windows'],['MacIntel','macos'],['Linux x86_64','linux']]) {
    const r=await run(platform,good);assert.match(r.selected,new RegExp(expected));assert.equal(r.badge.hidden,false);assert.equal(r.label.textContent,'v0.3.393-nightly.test');
  }
  const mobile=await run('Linux armv8l',good,'Android');assert.equal(mobile.selected,undefined);
  const ipad=await run('MacIntel',good,'',5);assert.equal(ipad.selected,undefined);
  const failure=await run('Win32',{ok:false});assert.equal(failure.label.href,'fallback');assert.match(failure.date.textContent,/See releases/);
  const bad=await run('MacIntel',{ok:true,json:async()=>({tag_name:'bad',html_url:'https://evil.example/DerpcatMusic/KONTRA/releases/tag/bad'})});assert.equal(bad.label.href,'fallback');
  console.log('Release lookup: platform detection, Android exclusion, version display, API failure and URL validation pass.');
})();
