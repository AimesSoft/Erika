// Verify native WebKit popovers against the actual app UI using its simulated clock.
import assert from 'node:assert/strict';
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import {checkPlayerMenus} from './menu-regression.mjs';
import {checkDanmakuComposer} from './composer-regression.mjs';
const {webkit} = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const output = process.env.ERIKA_APP_REPORT_DIR || 'target/web-settings-webkit-review';
const base = (process.env.ERIKA_APP_URL || 'http://127.0.0.1:18766').replace(/\/$/, '');
const appRoot = process.env.ERIKA_APP_ROOT || 'target/bern/web_app';
await mkdir(output, {recursive:true});
const source = await readFile(`${appRoot}/app.js`,'utf8');
const commands = Object.fromEntries([...source.matchAll(/^  (\w+): \(ctx[^\n]+?B.dispatch\(ctx, (\d+),/gm)].map(m=>[m[1],Number(m[2])]));
const browser = await webkit.launch({headless:true});
const page = await browser.newPage({viewport:{width:1280,height:900},hasTouch:true,reducedMotion:'reduce'});
const errors=[];
page.on('pageerror',e=>errors.push(String(e)));
page.on('console',m=>{if(m.type()==='error' && m.text().startsWith('[bern]')) errors.push(m.text());});
try {
  await page.goto(`${base}/`);
  await page.waitForFunction(()=>!!window.bern);
  await page.evaluate(commands=>{
    window.command=(name,args=[])=>new Promise((resolve,reject)=>bern.host.dispatch(commands[name],w=>{
      for(const [type,value] of args) w[type](type==='i64'?BigInt(value):value);
    },null,e=>e?reject(new Error(e.message)):resolve()));
  },commands);
  for(let i=0;i<4;i++) await page.evaluate(()=>command('onboarding_next'));
  await page.evaluate(()=>command('play', [['text','Popover regression'],['i64',1],['text',''],['f64',0],['text','sample:popover-regression']]));
  const settings=page.getByRole('button',{name:'播放设置',exact:true});
  const panel=page.getByRole('dialog',{name:'播放设置',exact:true});
  await page.evaluate(()=>command('toggle_pause'));
  for(const viewport of [{width:1280,height:900},{width:844,height:390}]) {
    await page.setViewportSize(viewport);
    const play=page.getByRole('button',{name:'播放',exact:true});
    const pause=page.getByRole('button',{name:'暂停',exact:true});
    await play.waitFor();
    for(const touch of [false,true]) {
      if (viewport.width === 1280) await checkDanmakuComposer(page, touch);
      await checkPlayerMenus(page, touch);
      const outside=()=>touch?page.touchscreen.tap(300,200):page.mouse.click(300,200);
      await settings.click();
      await panel.waitFor();
      await panel.getByRole('radio',{name:'音频',exact:true}).click();
      await page.evaluate(async()=>{
        for(let i=0;i<5;i++) {
          await command('player_panel',[['u8',0]]);
          await command('player_panel',[['u8',1]]);
          await command('player_tick');
        }
      });
      assert.equal(await panel.getByText('播放设置',{exact:true}).count(),1);
      await outside();
      await panel.waitFor({state:'hidden'});
      await play.waitFor();
      await outside();
      await pause.waitFor();
      await settings.click();
      await panel.waitFor();
      await outside();
      await panel.waitFor({state:'hidden'});
      await pause.waitFor();
      await outside();
      await play.waitFor();
      await settings.click();
      await panel.waitFor();
      await page.screenshot({path:`${output}/settings-${viewport.width}-${touch?'touch':'mouse'}.png`});
      await page.keyboard.press('Escape');
      await panel.waitFor({state:'hidden'});
    }
  }
  assert.deepEqual(errors,[]);
  const report={browser:browser.version(),playback:'app simulated clock (no GPU decoding)',viewports:['1280x900','844x390'],inputs:['mouse','touch'],checks:['desktop composer outside press blurs without toggling; draft, shortcuts and Send work','settings/subtitle/speed/more dismissal preserves play/pause','next click toggles','rapid close/reopen stays unique','inside controls work','Escape closes'],errors};
  await writeFile(`${output}/report.json`,JSON.stringify(report,null,2));
  console.log('WebKit settings passed',report);
} catch(error) {
  console.error(await page.locator('body').innerText().catch(()=>''));
  await page.screenshot({path:`${output}/failure.png`});
  throw error;
} finally {await browser.close();}
