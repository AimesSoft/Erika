import assert from 'node:assert/strict';

// Start and finish paused. Only the desktop layout currently has a composer.
export async function checkDanmakuComposer(page, touch = false) {
  const input = page.locator('[data-bern-target="player_controls.danmaku_text"]');
  const play = page.locator('[data-bern-target="player_controls.play"]');
  const pause = page.locator('[data-bern-target="player_controls.pause"]');
  const outside = () => touch ? page.touchscreen.tap(300, 200) : page.mouse.click(300, 200);
  const press = locator => touch ? locator.tap() : locator.click();
  const isFocused = () => input.evaluate(node => document.activeElement === node);
  const draft = '未发送的弹幕 draft';
  await play.waitFor();
  for (const state of [play, pause]) {
    await input.fill(draft);
    await press(input);
    assert.equal(await isFocused(), true, 'editing inside the composer keeps focus');
    await input.evaluate(node => node.setSelectionRange(node.value.length, node.value.length));
    await page.keyboard.press('Space');
    assert.equal(await input.inputValue(), `${draft} `);
    await state.waitFor();
    await outside();
    assert.equal(await isFocused(), false, 'outside press blurs the composer');
    assert.equal(await input.inputValue(), `${draft} `, 'blur preserves the draft');
    await state.waitFor();
    await outside();
    await (state === play ? pause : play).waitFor();
  }
  // Other controls also require a new press after dismissing the composer.
  await input.focus();
  await press(play);
  assert.equal(await isFocused(), false);
  await play.waitFor();
  await press(play);
  await pause.waitFor();
  await page.keyboard.press('Space');
  await play.waitFor();
  // Send is part of the composer. Observe its click before the app handler so
  // this regression never publishes a comment or depends on an online account.
  const send = page.locator('[data-bern-target="player_controls.send"]');
  await send.evaluate(node => {
    window.composerSendClicked = false;
    node.addEventListener('click', event => {
      window.composerSendClicked = true;
      event.preventDefault();
      event.stopImmediatePropagation();
    }, {capture:true, once:true});
  });
  await input.focus();
  await press(send);
  assert.equal(await page.evaluate(() => window.composerSendClicked), true, 'Send remains clickable');
  assert.equal(await input.inputValue(), `${draft} `);
  await input.fill('');
  await outside();
  await play.waitFor();
}
