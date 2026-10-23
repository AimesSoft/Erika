// Run against a built native WebUI host. ERIKA_WEBUI_URL includes its token.
import assert from 'node:assert/strict';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const url = process.env.ERIKA_WEBUI_URL || process.env.ERIKA_WEBAPP_URL;
const remote = !!process.env.ERIKA_WEBUI_URL;
assert.ok(url, 'Set ERIKA_WEBUI_URL to the native host URL, including #token=');
const root = `target/bern/${remote ? 'web_ui' : 'web_app'}`, out = `target/${remote ? 'webui' : 'webapp'}-playback-review`;
const fixtures = process.env.LIBERIKA_FIXTURES || '../liberika/target/web/fixtures';
const source = await readFile(`${root}/app.js`, 'utf8');
assert.match(source, remote ? /mode: "ui"/ : /mode: "app"/);
const commands = Object.fromEntries([...source.matchAll(/^  (\w+): \(ctx[^\n]+?B.dispatch\(ctx, (\d+),/gm)].map(m => [m[1], Number(m[2])]));
await mkdir(out, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true,
  args: process.env.ERIKA_SOFTWARE_WEBGL === '1' ? ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] : [] });
const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
const errors = [], report = { deployment: remote ? 'native-webui' : 'web-app', browser: browser.version() };
async function setup(page) {
  page.on('pageerror', error => errors.push(String(error)));
  await page.goto(url);
  await page.waitForFunction(() => !!window.bern, null, { timeout: 30000 });
  await page.evaluate(async commands => {
    window.command = (name, args = []) => new Promise((resolve, reject) => bern.host.dispatch(commands[name], w => {
      for (const [type, value] of args) w[type](type === 'i64' ? BigInt(value) : value);
    }, null, e => e ? reject(new Error(e.message)) : resolve()));
    const { BrowserPlayer } = await import('./extension/player.js');
    const ensure = BrowserPlayer.prototype.ensure;
    BrowserPlayer.prototype.ensure = async function () { window.testPlayer = this; return ensure.call(this); };
    const runtimeURL = performance.getEntriesByType('resource').find(e => /\/bern.js\?/.test(e.name)).name;
    window.runtime = await import(runtimeURL);
  }, commands);
}
const command = (page, name, args = []) => page.evaluate(([name, args]) => command(name, args), [name, args]);
const wait = (page, fn) => page.waitForFunction(fn, null, { timeout: 30000 });
async function pick(page, path) {
  const chooser = page.waitForEvent('filechooser');
  chooser.catch(() => {});
  await page.locator('[data-bern-target$=".open_file"], [data-bern-target$=".pure_open_file"]').click();
  await (await chooser).setFiles(path);
}
async function local(page, name, data, action) {
  await page.evaluate(async ({name, data, action}) => {
    const file = new File([data], name), url = URL.createObjectURL(file);
    runtime.pageFiles.set(url, file);
    await runtime.prepareFile(url);
    await command(action, [['text', url]]);
  }, {name, data, action});
}
const page = await context.newPage();
let engine;
function native(...args) {
  const output = execFileSync(`${root}/bin/erika`, ['--engine', engine, ...args], {encoding: 'utf8'}).trim();
  return output ? JSON.parse(output) : null;
}
const state = () => native('state').values;
try {
  await setup(page);
  engine = await page.evaluate(() => bern.link.engineID);
  report.engine = engine;
  await command(page, 'onboarding_skip');
  await command(page, 'onboarding_next');
  await command(page, 'close_player');
  await pick(page, `${fixtures}/test.mkv`);
  await wait(page, () => window.testPlayer?.snapshot().picture && testPlayer.instance.currentTime > .7);
  await wait(page, () => testPlayer.instance.stats.subtitleImages > 0);
  await page.waitForTimeout(400);
  if (remote) {
    const s = state();
    assert.equal(s.video, true); assert.equal(s.title, 'test.mkv');
    assert.ok(s.elapsed > 0 && s.duration > 5, JSON.stringify({elapsed:s.elapsed,duration:s.duration}));
    assert.ok(s.player.subtitle_tracks.length > 0);
    report.native = { title:s.title, elapsed:s.elapsed, duration:s.duration };
  }
  report.local = await page.evaluate(() => testPlayer.snapshot());
  await page.screenshot({path: `${out}/local-mkv.png`});

  // Drive the native engine over IPC and observe the actual browser decoder.
  if (remote) native('erika', 'toggle-pause');
  else await command(page, 'toggle_pause');
  await wait(page, () => !testPlayer.instance.playing);
  const paused = await page.evaluate(() => testPlayer.instance.currentTime);
  await page.waitForTimeout(350);
  assert.ok(Math.abs(await page.evaluate(() => testPlayer.instance.currentTime) - paused) < .1);
  if (remote) native('erika', 'seek', '--progress', '0.4');
  else await command(page, 'seek', [['f64', .4]]);
  await wait(page, () => testPlayer.instance.currentTime > 4 && !testPlayer.instance.playing);
  report.seek = await page.evaluate(() => testPlayer.instance.currentTime);
  assert.ok(report.seek < 6);
  await local(page, 'local.ass', await readFile(`${fixtures}/test.ass`, 'utf8'), 'attach_subtitle');
  await wait(page, () => testPlayer.snapshot().subtitleTracks.some(t => t.name === 'local.ass'));
  await local(page, 'local.json', JSON.stringify([{time:0,text:'WebUI 本地弹幕',color:'#ff0000'}]), 'mount_danmaku');
  await page.waitForTimeout(600);
  if (remote) assert.equal(state().player.danmaku_count, 1);
  assert.equal(await page.evaluate(() => testPlayer.results.length), 0);
  const download = page.waitForEvent('download');
  await command(page, 'screenshot');
  const image = await download; await image.saveAs(`${out}/capture.png`);
  report.screenshot = image.suggestedFilename();

  // Another UI may display the same player without stealing its local File.
  const second = remote ? await context.newPage() : page;
  if (remote) await setup(second);
  await command(page, 'toggle_pause');
  await wait(page, () => testPlayer.instance.playing);
  await command(second, 'close_player');
  await pick(second, `${fixtures}/test.webm`);
  await wait(second, () => window.testPlayer?.snapshot().picture && testPlayer.instance.currentTime > .5);
  if (remote) await wait(page, () => !testPlayer.source && !testPlayer.instance.playing);
  report.secondPage = await second.evaluate(() => testPlayer.snapshot());
  assert.match(report.secondPage.video, /VP9/);

  await command(second, 'close_player');
  await local(second, 'bad.mkv', 'invalid media', 'open_file');
  await wait(second, () => !!testPlayer.error);
  assert.ok(await second.locator('.bern-video-status:visible').innerText());
  await command(second, 'close_player');
  await pick(second, `${fixtures}/test.mkv`);
  await wait(second, () => testPlayer.snapshot().picture && !testPlayer.error && testPlayer.instance.playing);
  report.recovered = true;
  await command(second, 'close_player');
  await second.close();
  assert.deepEqual(errors, []);
  report.errors = errors;
  await writeFile(`${out}/report.json`, JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
} catch (error) {
  console.error('Playback', await page.evaluate(() => window.testPlayer?.snapshot()).catch(() => null));
  console.error('Errors', errors);
  await page.screenshot({path:`${out}/failure.png`}).catch(() => {});
  throw error;
} finally { await browser.close(); }
