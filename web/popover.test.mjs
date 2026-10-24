// Real playback + native WebUI popup regression; use a private host per run.
import assert from 'node:assert/strict';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
const { chromium, webkit } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const remote = !process.env.ERIKA_WEBAPP_URL;
const browserType = process.env.ERIKA_TEST_BROWSER || 'chromium';
const firstFrameOnly = process.env.ERIKA_POPOVER_FIRST_FRAME_ONLY === '1';
const root = `target/bern/${remote ? 'web_ui' : 'web_app'}`;
const out = `target/web-popover-review/${remote ? 'ui' : 'app'}-${browserType}${firstFrameOnly ? '-first-frame' : ''}`;
await mkdir(out, {recursive:true});
let host, url = process.env.ERIKA_WEBAPP_URL;
if (remote) {
  host = spawn(`${root}/bin/erika-host`, ['--port', '0'], {stdio:['ignore','pipe','pipe']});
  url = await new Promise((resolve, reject) => {
    let output = '';
    const timer = setTimeout(() => reject(new Error('Host startup timed out')), 15000);
    host.on('exit', code => reject(new Error(`Host exited ${code}`)));
    host.stdout.on('data', bytes => {
      output += bytes;
      const found = output.match(/^open (\S+)/m);
      if (found) {clearTimeout(timer);resolve(found[1]);}
    });
  });
}
const generated = await readFile(`${root}/app.js`, 'utf8');
const commands = Object.fromEntries([...generated.matchAll(/^  (\w+): \(ctx[^\n]+?B.dispatch\(ctx, (\d+),/gm)].map(m => [m[1],Number(m[2])]));
const browser = await (browserType === 'webkit' ? webkit : chromium).launch({headless:true,
  ...(browserType === 'chromium' ? {channel:'chrome',args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']} : {})});
const page = await browser.newPage({viewport:{width:1280,height:800},hasTouch:true});
page.setDefaultTimeout(15000);
const errors = [], report = {mode:remote?'ui':'app',browser:browser.version(),checks:[]};
page.on('pageerror', e => errors.push(String(e)));
const command = (name, args = []) => page.evaluate(([name,args]) => command(name,args),[name,args]);
const assertPictureUndimmed = async (name, expectActive = true) => {
  const frames = await page.getByRole('button',{name:'播放或暂停画面',exact:true}).evaluate(async picture => {
    const frames = [];
    for (let i=0;i<4;i++) {
      const style = getComputedStyle(picture);
      frames.push({active:picture.matches(':active'), filter:style.filter, opacity:style.opacity});
      await new Promise(requestAnimationFrame);
    }
    return frames;
  });
  if (expectActive) assert.ok(frames.some(frame=>frame.active), 'held outside press reaches the picture active state');
  assert.ok(frames.every(frame=>frame.filter==='none' && frame.opacity==='1'),
    `dismissing ${name} must not dim the picture: ${JSON.stringify(frames)}`);
};
try {
  if (firstFrameOnly) await page.addInitScript(() => {
    // Native toggle is a deferred task and can arrive after a paint. Stretch
    // that gap to reproduce the full-panel flash deterministically as well
    // as sampling the browser's normal event scheduling below.
    window.toggleDelay = 0;
    const add = document.addEventListener;
    document.addEventListener = function(type, listener, options) {
      return add.call(this, type, type === 'toggle' ? event => {
        if (toggleDelay) setTimeout(() => listener.call(this, event), toggleDelay);
        else listener.call(this, event);
      } : listener, options);
    };
  });
  await page.goto(url);
  await page.waitForFunction(() => !!window.bern);
  await page.evaluate(async commands => {
    // Delay only settings commands, while real playback/state updates keep
    // arriving. This reproduces the stale controlled-popover record in WebUI
    // and also exercises asynchronous WASM replies in the pure Web build.
    const dispatch = bern.host.dispatch.bind(bern.host);
    window.pendingPanelCommands = 0;
    bern.host.dispatch = (index, encode, win, done) => {
      if (index === commands.player_panel) {
        pendingPanelCommands++;
        setTimeout(() => dispatch(index, encode, win, (...args) => {
          pendingPanelCommands--;
          done?.(...args);
        }), 250);
      } else dispatch(index, encode, win, done);
    };
    window.command = (name, args = []) => new Promise((resolve,reject) => bern.host.dispatch(commands[name], w => {
      for (const [type,value] of args) w[type](type==='i64'?BigInt(value):value);
    },null,error => error?reject(new Error(error.message)):resolve()));
    window.popupEvents = [];
    document.addEventListener('beforetoggle', e => {
      if (e.target.hasAttribute('popover')) popupEvents.push({name:e.target.getAttribute('aria-label'),state:e.newState,children:e.target.childElementCount});
    },true);
    const {BrowserPlayer} = await import('./extension/player.js');
    const ensure = BrowserPlayer.prototype.ensure;
    BrowserPlayer.prototype.ensure = async function() {window.testPlayer=this;return ensure.call(this);};
  },commands);
  await command('onboarding_skip'); await command('onboarding_next');
  if (browserType === 'webkit') {
    await command('play', [['text','Popup regression'],['i64',1],['text',''],['f64',0],['text','sample:popup']]);
  } else {
    const pick = page.waitForEvent('filechooser'); pick.catch(()=>{});
    await page.getByRole('button',{name:'打开文件',exact:true}).click();
    await (await pick).setFiles('../liberika/target/web/fixtures/test.mkv');
    await page.waitForFunction(() => window.testPlayer?.snapshot().picture && testPlayer.instance.currentTime>.5);
  }
  await command('toggle_pause');
  await page.getByRole('button',{name:'播放',exact:true}).waitFor();
  if (firstFrameOnly) {
    report.frames = [];
    for (const reducedMotion of ['no-preference', 'reduce']) {
      await page.emulateMedia({reducedMotion});
      for (const delay of [0, 150]) {
        await page.evaluate(delay => { window.toggleDelay = delay; }, delay);
        for (const name of ['字幕与音轨', '音量', '播放设置']) {
          const trigger = page.getByRole('button',{name,exact:true});
          const frames = await trigger.evaluate(async trigger => {
            const panel = document.getElementById(trigger.getAttribute('popovertarget'));
            const frames = [], start = performance.now();
            const sample = () => {
              const style = getComputedStyle(panel);
              frames.push({ms:performance.now()-start, open:panel.matches(':popover-open'),
                opacity:Number(style.opacity), transform:style.transform, animations:panel.getAnimations().length});
            };
            trigger.click();
            sample(); // The state exposed to a paint before deferred toggle.
            await new Promise(resolve => {
              const frame = () => {
                sample();
                if (performance.now()-start < 550) requestAnimationFrame(frame);
                else resolve();
              };
              requestAnimationFrame(frame);
            });
            return frames;
          });
          report.frames.push({name, delay, reducedMotion, frames});
          assert.equal(frames[0].open, true);
          assert.equal(frames[0].opacity, 0, `${name}: the first visible state must be the entrance start, not the full panel`);
          assert.ok(frames.every(f=>f.open), `${name}: entrance must not close/reopen the native popover`);
          for (let i=1;i<frames.length;i++) assert.ok(frames[i].opacity+.01 >= frames[i-1].opacity,
            `${name}: opacity dropped from ${frames[i-1].opacity} to ${frames[i].opacity} at ${frames[i].ms} ms`);
          assert.ok(frames.at(-1).opacity > .99, `${name}: entrance finishes fully visible`);
          await page.screenshot({path:`${out}/${name}-${reducedMotion}-${delay}.png`});
          await page.keyboard.press('Escape');
          await page.waitForFunction(() => pendingPanelCommands === 0);
          // Flush a delayed closing toggle before the next test case.
          if (delay) await page.waitForTimeout(delay+20);
          report.checks.push(`${name}: ${reducedMotion}, toggle delay ${delay} ms, first paint and monotonic fade`);
        }
      }
    }
  } else {
  assert.equal(await page.getByRole('button',{name:'更多',exact:true}).count(),0);
  const screenshot = page.getByRole('button',{name:'截图',exact:true});
  assert.equal(await screenshot.getAttribute('popovertarget'),null);
  if (browserType === 'chromium') {
    const download = page.waitForEvent('download');
    await screenshot.click();
    const capture = await download;
    await capture.saveAs(`${out}/screenshot-button.png`);
    assert.equal(await capture.failure(),null);
    report.checks.push('screenshot button downloads the current picture directly');
  }
  // Use the actual Fullscreen API. Each cycle checks both popup bindings
  // (session volume and engine settings), including playback after idle.
  for (const playing of [false, true]) {
    await page.getByRole('button',{name:'全屏',exact:true}).click();
    await page.waitForFunction(() => !!document.fullscreenElement);
    const viewport = playing ? {width:1920,height:1080} : {width:1280,height:800};
    await page.setViewportSize(viewport);
    if (playing) {
      const wakeTrigger = page.getByRole('button',{name:'音量',exact:true});
      await wakeTrigger.hover();
      const wake = await wakeTrigger.boundingBox();
      await command('seek', [['f64',0.1]]);
      await command('toggle_pause');
      await page.getByRole('button',{name:'暂停',exact:true}).waitFor();
      await page.mouse.move(viewport.width/2,viewport.height/2);
      await page.waitForTimeout(1200);
      await page.mouse.move(wake.x+wake.width/2,wake.y+wake.height/2);
      // The wake event initially hits the picture while controls are hidden.
      // After their entrance, a stationary pointer over the button must keep
      // it available beyond the picture's one-second auto-hide timeout.
      await page.waitForTimeout(1500);
      assert.equal(await wakeTrigger.isVisible(),true, 'controls stay awake under a stationary mouse');
    }
    for (const name of ['音量','播放设置']) {
      const trigger = page.getByRole('button',{name,exact:true});
      await trigger.click();
      const panel = page.getByRole('dialog',{name,exact:true});
      await panel.waitFor({state:'visible',timeout:5000});
      await page.waitForTimeout(700);
      assert.equal(await panel.evaluate(panel => {
        const r = panel.getBoundingClientRect();
        return !!document.fullscreenElement?.contains(panel)
          && r.left >= 0 && r.top >= 0 && r.right <= innerWidth+1 && r.bottom <= innerHeight+1
          && panel.contains(document.elementFromPoint(r.left+20,r.top+20));
      }),true, `${name}: fullscreen popup is visible and receives pointer input`);
      if (name === '播放设置') await panel.getByRole('radio',{name:'音频',exact:true}).click();
      await panel.getByRole('slider',{name:'音量',exact:true}).press('ArrowLeft');
      await page.screenshot({path:`${out}/fullscreen-${playing?'playing':'paused'}-${name}.png`});
      await page.mouse.move(viewport.width/2,viewport.height/2);
      await page.mouse.down();
      try {
        await assertPictureUndimmed(`${name} (fullscreen)`);
      } finally {
        await page.mouse.up();
      }
      await panel.waitFor({state:'hidden'});
      await page.waitForFunction(() => pendingPanelCommands === 0);
      assert.equal(await page.evaluate(() => !!document.fullscreenElement),true);
    }
    if (playing) {
      await command('toggle_pause');
      await page.getByRole('button',{name:'播放',exact:true}).waitFor();
    }
    await page.getByRole('button',{name:'退出全屏',exact:true}).click();
    await page.waitForFunction(() => !document.fullscreenElement);
    await page.setViewportSize({width:1280,height:800});
    report.checks.push(`fullscreen ${playing?'playing after idle':'paused'}: volume/settings open, accept input, close and exit fullscreen`);
  }
  // Clicking a different popup trigger should switch in one press.
  const volume = page.getByRole('button',{name:'音量',exact:true});
  await volume.click();
  await page.getByRole('dialog',{name:'音量',exact:true}).waitFor();
  await page.getByRole('button',{name:'播放设置',exact:true}).click();
  await page.getByRole('dialog',{name:'播放设置',exact:true}).waitFor({state:'visible',timeout:5000});
  await page.keyboard.press('Escape');
  await page.waitForTimeout(600);
  assert.equal(await page.locator('[popover]:popover-open').count(),0);
  // A touch switches between popup triggers in one gesture as well.
  await volume.tap();
  await page.getByRole('dialog',{name:'音量',exact:true}).waitFor();
  await page.getByRole('button',{name:'播放设置',exact:true}).tap();
  await page.getByRole('dialog',{name:'播放设置',exact:true}).waitFor();
  await page.keyboard.press('Escape');
  await page.waitForTimeout(600);
  report.checks.push('mouse and touch switch volume to settings in one press');
  // Close/reopen before an engine acknowledgement or entrance animation ends.
  const rapid = page.getByRole('button',{name:'播放设置',exact:true});
  const eventStart = await page.evaluate(() => popupEvents.length);
  await rapid.evaluate(async trigger => {
    for (let i=0;i<5;i++) {
      trigger.click();
      await new Promise(requestAnimationFrame);
      trigger.click();
    }
  });
  await page.waitForTimeout(700);
  assert.deepEqual(await page.evaluate(start => popupEvents.slice(start).filter(e => e.name==='播放设置').map(e => e.state),eventStart),
    Array.from({length:5},()=>['open','closed']).flat(), 'no phantom reopening or duplicate entrance');
  assert.equal(await page.locator('[popover]:popover-open').count(),0);
  report.checks.push('rapid close/reopen survives delayed acknowledgements without remount or stale animation');
  for (const viewport of [{width:1280,height:800},{width:844,height:390}]) {
    await page.setViewportSize(viewport);
    for (const name of ['播放设置','音量']) {
      const trigger = page.getByRole('button',{name,exact:true});
      if (!(await trigger.count())) continue;
      for (let i=0;i<3;i++) {
        await trigger.click();
        const panel = page.locator(`#${await trigger.getAttribute('popovertarget')}`);
        await panel.waitFor({state:'visible',timeout:5000});
        await page.waitForTimeout(350);
        assert.equal(await panel.isVisible(),true, `${name} must stay open`);
        assert.equal(await panel.getByText(name,{exact:true}).count(),1, 'exactly one panel body');
        assert.equal(await page.locator('[popover]:popover-open').count(),1);
        if (name==='播放设置') {
          await panel.getByRole('radio',{name:'音频',exact:true}).click();
          await page.waitForTimeout(350);
          await page.screenshot({path:`${out}/${viewport.width}-${i}-open.png`});
        }
        // Dismissing a popup consumes the press, but the browser still sets
        // :active on the picture underneath. Its pressed:none style must
        // suppress the default content-button brightness effect throughout
        // that press, including before the controlled binding acknowledges it.
        await page.mouse.move(300,200);
        await page.mouse.down();
        try {
          await assertPictureUndimmed(name);
        } finally {
          await page.mouse.up();
        }
        await panel.waitFor({state:'hidden',timeout:5000});
        await page.waitForTimeout(350);
        assert.equal(await page.locator('[popover]:popover-open').count(),0, 'dismissed popup must stay closed');
        assert.equal(await panel.locator(':scope > *').count(),0, 'disposed panel leaves no content');
        assert.equal(await panel.evaluate(p=>p.getAnimations().length),0, 'closed panel retains no entrance animation');
        await page.getByRole('button',{name:'播放',exact:true}).waitFor();
        if (i===0) await page.screenshot({path:`${out}/${viewport.width}-${name}-closed.png`});
      }
      if (browserType === 'chromium') {
        // Hold a real touch too: touch has no mouse hover state to mask a
        // missing pressed:none override, and dismissal consumes its events.
        await trigger.tap();
        const panel = page.getByRole('dialog',{name,exact:true});
        await panel.waitFor();
        const touch = await page.context().newCDPSession(page);
        try {
          await touch.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:300,y:200}]});
          // Some engines suppress touch :active after preventDefault. The
          // visual result must still stay unchanged whether it applies or not.
          await assertPictureUndimmed(`${name} (touch)`, false);
        } finally {
          await touch.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
          await touch.detach();
        }
        await panel.waitFor({state:'hidden'});
        await page.getByRole('button',{name:'播放',exact:true}).waitFor();
        await page.waitForFunction(() => pendingPanelCommands === 0);
      }
      report.checks.push(`${viewport.width}: ${name} open, internal controls, dismiss without dimming, no duplicate content`);
    }
    const settings = page.getByRole('button',{name:'播放设置',exact:true});
    await command('toggle_pause');
    // A viewer moves from the picture to the toolbar before pressing it.
    // Programmatic playback alone does not wake an already idle toolbar.
    await page.mouse.move(viewport.width-105,55);
    await page.getByRole('button',{name:'暂停',exact:true}).waitFor();
    const start = await page.evaluate(() => popupEvents.length);
    await settings.click();
    const panel = page.getByRole('dialog',{name:'播放设置',exact:true});
    await panel.waitFor({state:'visible',timeout:5000});
    await page.waitForTimeout(800);
    assert.equal(await panel.isVisible(),true, 'playing updates must not close settings');
    assert.deepEqual(await page.evaluate(start => popupEvents.slice(start).filter(e=>e.name==='播放设置').map(e=>e.state),start),['open'],
      'playing state updates do not close and remount settings');
    await page.keyboard.press('Escape');
    await panel.waitFor({state:'hidden'});
    await command('toggle_pause');
    await page.getByRole('button',{name:'播放',exact:true}).waitFor();
    // Finish this interaction before rebuilding the responsive layout.
    await page.waitForFunction(() => pendingPanelCommands === 0);
    report.checks.push(`${viewport.width}: settings stays mounted during playing updates and dismisses with Escape`);
  }
  }
  assert.deepEqual(errors,[]);
} catch (error) {
  report.failure=String(error);
  await page.screenshot({path:`${out}/failure.png`}).catch(()=>{});
  throw error;
} finally {
  report.errors=errors;
  report.events=await page.evaluate(()=>window.popupEvents).catch(()=>[]);
  await writeFile(`${out}/report.json`,JSON.stringify(report,null,2));
  await browser.close();host?.kill();
}
console.log(JSON.stringify(report,null,2));
