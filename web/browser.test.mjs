// Build Erika web_app, serve target/bern/web_app, and set PLAYWRIGHT_MODULE.
import assert from 'node:assert/strict';
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import {checkPlayerMenus} from './menu-regression.mjs';
import {checkDanmakuComposer} from './composer-regression.mjs';
const {chromium} = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const base = (process.env.ERIKA_APP_URL || 'http://127.0.0.1:18766').replace(/\/$/, '');
const fixtures = process.env.LIBERIKA_FIXTURES || '../liberika/target/web/fixtures';
const output = process.env.ERIKA_APP_REPORT_DIR || 'target/web-review';
const appRoot = process.env.ERIKA_APP_ROOT || 'target/bern/web_app';
const staticOnly = process.env.ERIKA_STATIC_ONLY === '1';
await mkdir(output, {recursive:true});
// Read the exact deployed application, including when testing an unpacked ZIP.
const generated = await readFile(`${appRoot}/app.js`, 'utf8');
if (staticOnly) assert.match(generated, /mode: "app"/);
const commands = Object.fromEntries([...generated.matchAll(/^  (\w+): \(ctx[^\n]+?B.dispatch\(ctx, (\d+),/gm)].map(m=>[m[1],Number(m[2])]));
const browser = await chromium.launch({channel:'chrome',headless:true,
  args: process.env.ERIKA_SOFTWARE_WEBGL === '1' ? ['--use-angle=swiftshader','--enable-unsafe-swiftshader'] : [],
});
const page=await browser.newPage({viewport:{width:1280,height:900}}), errors=[], runtimeErrors=[], failedRequests=[], fontRequests=[], externalRequests=[], sockets=[];
if (staticOnly) {
  const origin = new URL(base).origin;
  await page.context().route('**/*', route => {
    const url = new URL(route.request().url());
    if (['http:', 'https:'].includes(url.protocol) && url.origin !== origin) {
      externalRequests.push(url.href);
      return route.abort();
    }
    return route.continue();
  });
  await page.context().routeWebSocket('**/*', socket => {
    sockets.push(socket.url());
    socket.close();
  });
}
page.on('request',request=>{if(new URL(request.url()).pathname.endsWith('/subfont.ttf'))fontRequests.push(request.url());});
page.on('requestfailed',request=>{failedRequests.push({url:request.url(),range:request.headers().range,error:request.failure()?.errorText});});
page.on('pageerror',e=>{errors.push(String(e));console.error(e);});
page.on('console',m=>{if(m.type()==='error') { console.error(m.text()); if(m.text().startsWith('[bern]')) runtimeErrors.push(m.text()); }});
const report={browser:browser.version(), deployment:staticOnly ? 'static-app' : 'app', url:base};
const deadline=setTimeout(()=>browser.close(),120000);
async function command(name, args=[]) {
  await page.evaluate(([name,args])=>window.command(name,args),[name,args]);
}
async function wait(fn) { await page.waitForFunction(fn,null,{timeout:20000}); }
try {
  await page.goto(`${base}/`);
  await wait(()=>!!window.bern);
  await page.evaluate(async commands=>{
    window.command=(name,args=[])=>new Promise((resolve,reject)=>bern.host.dispatch(commands[name],w=>{
      for(const [type,value] of args) w[type](type==='i64'?BigInt(value):value);
    },null,e=>e?reject(new Error(e.message)):resolve()));
    const {BrowserPlayer}=await import('./extension/player.js');
    const original=BrowserPlayer.prototype.ensure;
    BrowserPlayer.prototype.ensure=async function(){window.testPlayer=this;return original.call(this)};
    const bernURL=performance.getEntriesByType('resource').find(e=>/\/bern.js\?/.test(e.name)).name;
    window.testRuntime=await import(bernURL);
  },commands);
  for(let i=0;i<4;i++) await command('onboarding_next');
  // Open via Erika's actual file picker.
  const picker=page.waitForEvent('filechooser');
  picker.catch(()=>{});
  await page.locator('[data-bern-target$=".open_file"]').click();
  await (await picker).setFiles(`${fixtures}/test.mkv`);
  await wait(()=>window.testPlayer?.snapshot().picture && testPlayer.instance.currentTime>0.5);
  await wait(()=>testPlayer.instance.stats.subtitleImages>0);
  const dimensions=await page.evaluate(()=>testPlayer.canvas.getBoundingClientRect().toJSON());
  assert.ok(dimensions.width>600&&dimensions.height>300, JSON.stringify(dimensions));
  report.local=await page.evaluate(()=>({state:testPlayer.snapshot(),stats:testPlayer.instance.stats}));
  assert.equal(report.local.state.video.includes('WebCodecs'),true);
  assert.ok(report.local.state.subtitleTracks.length);
  // Import a synthetic local danmaku file through the same registered File API.
  await page.evaluate(async()=>{
    const data=JSON.stringify(Array.from({length:2000},(_,i)=>({time:i*0.03,text:['纯白 White','红色 Red','黑色 Black'][i%3]+i,color:['#ffffff','#ff0000','#000000'][i%3]})));
    const file=new File([data],'web-comments.json'),url=URL.createObjectURL(file);
    testRuntime.pageFiles.set(url,file);
    await command('mount_danmaku',[['text',url]]);
  });
  await wait(()=>testPlayer.instance.stats.glyphs>0);
  await command('toggle_pause');
  await wait(()=>!testPlayer.instance.playing);
  const paused=await page.evaluate(()=>testPlayer.instance.currentTime);
  await page.waitForTimeout(300);
  assert.ok(Math.abs(await page.evaluate(()=>testPlayer.instance.currentTime)-paused)<0.05);
  // Exercise the actual range input and its change/commit path, not only seek().
  await page.mouse.move(640, 760);
  const timeline = page.getByRole('slider', {name:'播放进度', exact:true});
  await timeline.waitFor({state:'visible'});
  const track = await timeline.boundingBox();
  const start = Number(await timeline.inputValue());
  await page.mouse.move(track.x + 8 + (track.width - 16) * start, track.y + track.height / 2);
  await page.mouse.down();
  await page.mouse.move(track.x + 8 + (track.width - 16) * 0.4, track.y + track.height / 2, {steps:10});
  await page.mouse.up();
  await wait(()=>testPlayer.instance.currentTime>4.5);
  assert.equal(await page.evaluate(()=>testPlayer.instance.playing),false);
  report.sliderSeek = await page.evaluate(()=>testPlayer.instance.currentTime);
  assert.ok(report.sliderSeek < 5.3, `slider seek: ${report.sliderSeek}`);
  await checkDanmakuComposer(page);
  assert.equal(await page.evaluate(()=>testPlayer.instance.playing), false);
  report.composerDismissal = 'outside press only blurs; draft survives; next click and shortcuts work; Send remains clickable';
  await checkPlayerMenus(page);
  assert.equal(await page.evaluate(()=>testPlayer.instance.playing), false);
  report.menuDismissal = 'subtitle, speed and more menus consume outside clicks; next click works; choices and Escape work';
  await page.evaluate(()=>{window.originalCanvas=testPlayer.canvas;window.originalWorker=testPlayer.instance.worker;});
  // Dismissing settings must consume the press, even while player state updates.
  const settings = page.getByRole('button', {name:'播放设置', exact:true});
  const panel = page.getByRole('dialog', {name:'播放设置', exact:true});
  for (const playing of [false, true, false]) {
    if (await page.evaluate(()=>testPlayer.instance.playing) !== playing) await command('toggle_pause');
    await page.waitForFunction(value=>testPlayer.instance.playing === value, playing);
    await settings.click();
    await panel.waitFor({state:'visible'});
    await command('player_tick');
    assert.equal(await panel.getByText('播放设置', {exact:true}).count(), 1);
    await page.mouse.click(300, 300);
    await panel.waitFor({state:'hidden'});
    await command('player_tick');
    assert.equal(await page.evaluate(()=>testPlayer.instance.playing), playing, 'dismissal must not toggle playback');
    await page.mouse.click(300, 300);
    await page.waitForFunction(value=>testPlayer.instance.playing === value, !playing);
  }
  if (await page.evaluate(()=>testPlayer.instance.playing)) await command('toggle_pause');
  for (const viewport of [{width:1280,height:900}, {width:844,height:390}]) {
    await page.setViewportSize(viewport);
    await settings.click();
    await panel.waitFor({state:'visible'});
    await panel.getByRole('radio', {name:'音频', exact:true}).click();
    assert.equal(await panel.getByText('播放设置', {exact:true}).count(), 1);
    assert.equal(await page.evaluate(()=>testPlayer.instance.playing), false, 'inside controls must not toggle playback');
    // A close/reopen can happen before the browser delivers its toggle event.
    await page.evaluate(async()=>{
      for (let i=0;i<4;i++) {
        await command('player_panel',[['u8',0]]);
        await command('player_panel',[['u8',1]]);
      }
    });
    assert.equal(await panel.getByText('播放设置', {exact:true}).count(), 1);
    await page.screenshot({path:`${output}/settings-${viewport.width}.png`});
    await page.keyboard.press('Escape');
    await panel.waitFor({state:'hidden'});
  }
  await page.setViewportSize({width:1280,height:900});
  report.popoverDismissal = 'outside press closes settings without toggling playback; next click works; no duplicate content; Escape closes';
  await command('player_panel',[['u8',1]]);
  await page.waitForTimeout(250);
  await command('player_panel',[['u8',0]]);
  await command('minimize');
  await page.waitForTimeout(250);
  await command('expand');
  await page.waitForTimeout(700);
  assert.equal(await page.evaluate(()=>testPlayer.canvas===originalCanvas&&testPlayer.instance.worker===originalWorker),true);
  // Continuous UI commands must settle at their last value without new player instances.
  for(let i=0;i<15;i++) {
    await command('set_danmaku_number',[['text','opacity'],['f64',0.4+i*0.04]]);
    await command('set_danmaku_number',[['text','font_size'],['f64',i/14]]);
  }
  await page.evaluate(()=>testPlayer.queue);
  assert.equal(await page.evaluate(()=>testPlayer.danmaku.fontSize),40);
  assert.ok(Math.abs(await page.evaluate(()=>testPlayer.danmaku.opacity)-0.96)<0.001);
  await command('set_danmaku_number',[['text','delay'],['f64',1.5]]);
  await command('set_player_number',[['text','volume'],['f64',0.25]]);
  assert.equal(await page.evaluate(()=>testPlayer.instance.gain.gain.value),0.25);
  await command('select_subtitle_track',[['i64',-1]]);
  await wait(()=>testPlayer.snapshot().subtitleTrack===null && testPlayer.instance.stats.subtitleImages===0);
  await page.evaluate(async()=>{
    const file=new File(['1\n00:00:00,000 --> 00:01:00,000\n外部字幕 External subtitle\n'],'external.srt'),url=URL.createObjectURL(file);
    testRuntime.pageFiles.set(url,file);await command('attach_subtitle',[['text',url]]);
  });
  await wait(()=>testPlayer.snapshot().subtitleTrack===1000000&&testPlayer.instance.stats.subtitleImages>0);
  await command('set_subtitle_supersampling',[['u8',0]]);
  await command('set_player_number',[['text','subtitle_size'],['f64',0.8]]);
  await command('select_audio_track',[['i64',report.local.state.audioTrack]]);
  await page.evaluate(()=>testPlayer.queue);
  await page.waitForTimeout(400);
  await page.screenshot({path:`${output}/player.png`});
  const pixels=await page.evaluate(async()=>{
    const blob=await testPlayer.instance.capture(320),image=await createImageBitmap(blob);
    const canvas=new OffscreenCanvas(image.width,image.height),ctx=canvas.getContext('2d');
    ctx.drawImage(image,0,0); image.close();
    const bytes=ctx.getImageData(0,0,canvas.width,canvas.height).data;
    let colored=0;for(let i=0;i<bytes.length;i+=4) if(Math.max(bytes[i],bytes[i+1],bytes[i+2])-Math.min(bytes[i],bytes[i+1],bytes[i+2])>80) colored++;
    return {width:canvas.width,height:canvas.height,colored};
  });
  assert.ok(pixels.width===320&&pixels.height>150&&pixels.colored>1000,JSON.stringify(pixels));
  report.pixels=pixels;
  await command('set_speed',[['u32',4]]);
  await page.evaluate(()=>testPlayer.queue);
  assert.equal(await page.evaluate(()=>testPlayer.instance.rate),1.5);
  const download=page.waitForEvent('download');
  await command('screenshot');
  await (await download).saveAs(`${output}/capture.png`);
  await command('toggle_pause');
  await wait(()=>testPlayer.instance.playing);
  report.configured=await page.evaluate(()=>({state:testPlayer.snapshot(),stats:testPlayer.instance.stats}));
  await command('close_player');
  await wait(()=>testPlayer.info===null && !testPlayer.instance.playing);
  assert.equal(await page.evaluate(()=>testPlayer.instance.sources.size),0);
  // Reopen a different codec and assert the subtitle list and comments were cleared.
  const bytes=await readFile(`${fixtures}/test.webm`);
  await page.evaluate(async bytes=>{
    const f=new File([new Uint8Array(bytes)],'test.webm'),url=URL.createObjectURL(f);
    testRuntime.pageFiles.set(url,f);await command('open_file',[['text',url]]);
  },Array.from(bytes));
  await wait(()=>testPlayer.info?.decoder.startsWith('vp09')&&testPlayer.instance.stats.hasVideo);
  report.webm=await page.evaluate(()=>testPlayer.snapshot());
  assert.equal(report.webm.subtitleTracks.length,0);
  await command('close_player');
  // HTTP Range path and real container chapters through the same app controls.
  await command('open_link', [['text', `${base}/test-fixtures/chapters.mkv`]]);
  await wait(()=>testPlayer.info?.chapters?.length===3&&testPlayer.instance.stats.hasVideo);
  report.remote=await page.evaluate(()=>testPlayer.snapshot());
  assert.equal(report.remote.chapters[0].title,'OP');
  await command('toggle_pause');
  await wait(()=>!testPlayer.instance.playing);
  await command('player_tick');
  // Chapter marks sit on the track center. Neither their full-width wrapper
  // nor an individual mark may intercept clicks or a drag across the track.
  report.chapterTimeline = [];
  for (const viewport of [{width:1280,height:900}, {width:844,height:390}]) {
    await page.setViewportSize(viewport);
    await timeline.waitFor({state:'visible'});
    await page.waitForTimeout(350); // Let the scene's resize/entrance motion settle before hit testing.
    const hitTargets = await timeline.evaluate(input=>{
      const r=input.getBoundingClientRect();
      const rail=input.closest('.bern-positioned').parentElement.querySelector('.bern-rail');
      const markers=rail?.querySelectorAll('.bern-canvas') ?? [];
      const points=[0.2,0.5,0.8].map(at=>({x:r.x+r.width*at,y:r.y+r.height/2}));
      for(const marker of markers) {
        const m=marker.getBoundingClientRect();
        points.push({x:m.x+m.width/2,y:m.y+m.height/2});
      }
      return points.map(p=>({point:p,target:document.elementFromPoint(p.x,p.y)?.outerHTML.slice(0,160),slider:document.elementFromPoint(p.x,p.y)===input}));
    });
    assert.ok(hitTargets.length >= 6, 'real chapter marks must be present');
    assert.ok(hitTargets.every(hit=>hit.slider),JSON.stringify(hitTargets));
    const r=await timeline.boundingBox();
    const thumb=await timeline.evaluate(input=>parseFloat(getComputedStyle(input).getPropertyValue('--bern-slider-thumb')));
    const scale=r.width/await timeline.evaluate(input=>input.offsetWidth);
    const x=at=>r.x+thumb*scale/2+(r.width-thumb*scale)*at;
    for(const at of [0.2,0.5,0.8]) {
      await page.mouse.click(x(at),r.y+r.height/2);
      await page.waitForFunction(at=>Math.abs(testPlayer.instance.currentTime/testPlayer.info.duration-at)<0.025,at);
      assert.equal(await page.evaluate(()=>testPlayer.instance.playing),false);
    }
    await page.mouse.move(x(0.8),r.y+r.height/2);
    await page.mouse.down();
    await page.mouse.move(x(0.35),r.y+r.height/2,{steps:12});
    await page.mouse.up();
    await page.waitForFunction(()=>Math.abs(testPlayer.instance.currentTime/testPlayer.info.duration-0.35)<0.025);
    report.chapterTimeline.push({viewport,hitTargets});
  }
  await page.setViewportSize({width:1280,height:900});
  await page.locator('[data-bern-target^="player_timeline.chapter"]').filter({hasText:'正文'}).click();
  await wait(()=>testPlayer.instance.currentTime>=3&&testPlayer.instance.currentTime<4);
  await command('seek_chapter',[['i64',1]]);
  await wait(()=>testPlayer.instance.currentTime>=3&&testPlayer.instance.currentTime<4);
  await page.setViewportSize({width:390,height:844});
  await page.waitForTimeout(700);
  assert.ok(await page.evaluate(()=>testPlayer.canvas.getBoundingClientRect().width>300));
  await page.screenshot({path:`${output}/phone.png`});
  await page.setViewportSize({width:844,height:390});
  await page.waitForTimeout(700);
  assert.ok(await page.evaluate(()=>testPlayer.canvas.getBoundingClientRect().width>600));
  await page.screenshot({path:`${output}/phone-landscape.png`});
  await page.setViewportSize({width:1280,height:900});
  // Bad input must fail visibly and the next open must recover.
  await page.evaluate(async()=>{
    const f=new File(['invalid media'],'bad.mkv'),url=URL.createObjectURL(f);
    testRuntime.pageFiles.set(url,f);await command('open_file',[['text',url]]);
  });
  await wait(()=>!!testPlayer.error);
  assert.ok(await page.locator('.bern-video-status').innerText());
  await command('open_link', [['text', `${base}/test-fixtures/chapters.mkv`]]);
  await wait(()=>testPlayer.snapshot().picture&&!testPlayer.error);
  await command('close_player');
  assert.deepEqual(errors,[]);
  assert.deepEqual(runtimeErrors,[]);
  report.fontRequests=fontRequests;
  assert.deepEqual(fontRequests,[],'fallback font must come from the Rust WASM');
  if (staticOnly) {
    report.externalRequests=externalRequests;
    report.webSockets=sockets;
    assert.deepEqual(externalRequests,[],'static playback must not depend on another origin');
    assert.deepEqual(sockets,[],'static playback must not connect to an engine host');
  }
  report.failedRequests=failedRequests;
  // Range probing cancels its body after reading Content-Range; close also aborts pending media reads.
  assert.deepEqual(failedRequests.filter(r=>!(r.error==='net::ERR_ABORTED'&&r.url.includes('/test-fixtures/'))),[]);
  await writeFile(`${output}/report.json`,JSON.stringify(report,null,2));
  console.log('Erika Web integration passed',JSON.stringify(report,null,2));
} catch(error) {
  console.error('BODY',await page.locator('body').innerText().catch(()=>''));
  console.error('STATE',await page.evaluate(()=>window.testPlayer?.snapshot()).catch(()=>null));
  await page.screenshot({path:`${output}/failure.png`}).catch(()=>{});
  throw error;
} finally {clearTimeout(deadline);await browser.close();}
