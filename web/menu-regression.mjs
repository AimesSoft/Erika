import assert from 'node:assert/strict';

// Start and finish paused. Exercise actual pointer input on the player menus
// and the picture, shared by the media integration and WebKit UI regressions.
export async function checkPlayerMenus(page, touch = false) {
  const play = page.locator('[data-bern-target="player_controls.play"], [data-bern-target="player_phone_scene.play_phone"]');
  const pause = page.locator('[data-bern-target="player_controls.pause"], [data-bern-target="player_phone_scene.pause_phone"]');
  const outside = () => touch ? page.touchscreen.tap(300, 200) : page.mouse.click(300, 200);
  const subtitles = page.locator('[data-bern-target="player_subtitle_menu.subtitles"], [data-bern-target="player_subtitle_menu.subtitles_off"]');
  const rate = page.getByRole('combobox', {name:'倍速', exact:true});
  const triggers = [subtitles, rate, page.getByRole('button', {name:'更多', exact:true})];
  for (const trigger of triggers) {
    await play.waitFor();
    for (const state of [play, pause]) {
      await trigger.click();
      const popup = page.locator(`#${await trigger.getAttribute('popovertarget')}`);
      await popup.waitFor({state:'visible'});
      await outside();
      await popup.waitFor({state:'hidden'});
      await state.waitFor();
      await outside();
      await (state === play ? pause : play).waitFor();
    }
    await trigger.click();
    const popup = page.locator(`#${await trigger.getAttribute('popovertarget')}`);
    await popup.waitFor({state:'visible'});
    await page.keyboard.press('Escape');
    await popup.waitFor({state:'hidden'});
    await play.waitFor();
  }
  // The dismissal guard must leave real choices and menu-to-panel handoff usable.
  await rate.click();
  await page.getByRole('option', {name:'1.5×', exact:true}).click();
  assert.match(await rate.innerText(), /1\.5×/);
  await play.waitFor();
  await rate.click();
  await page.getByRole('option', {name:'1.0×', exact:true}).click();
  await subtitles.click();
  await page.getByRole('menuitem', {name:'字幕轨道…', exact:true}).click();
  const settings = page.getByRole('dialog', {name:'播放设置', exact:true});
  await settings.waitFor({state:'visible'});
  await outside();
  await settings.waitFor({state:'hidden'});
  await play.waitFor();
}
