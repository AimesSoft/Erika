import test from 'node:test';
import assert from 'node:assert/strict';
import { RemotePlayback } from './remote.js';

function fixture() {
  const reports = [], commands = [];
  let response = {active:true, generation:1, commands:[]}, failure = false;
  const player = {results:[], operations:[], snapshot:() => ({source:'blob:local'}), fail:assert.fail};
  const api = {remote:{call:async (_, params) => {
    reports.push(JSON.parse(params.packet));
    if (failure) throw new Error('Disconnected');
    return JSON.stringify(response);
  }}, media:{stop:async () => commands.push('stop')}};
  const remote = new RemotePlayback(api, () => player, async message => commands.push(message.op));
  return {remote, player, reports, commands, response:v => response=v, failure:v => failure=v};
}

test('retries preserve reports and command IDs prevent duplicate execution', async () => {
  const f = fixture();
  const result = {op:'screenshot', name:'one.png'};
  f.player.operations.push(result);
  f.failure(true);
  await assert.rejects(f.remote.exchange());
  f.failure(false);
  f.response({active:true,generation:1,commands:[{id:1,message:{op:'capture'}}]});
  await f.remote.exchange(); await f.remote.queue;
  assert.deepEqual(f.reports[0], f.reports[1]);
  assert.equal(f.player.operations.length, 0);
  await f.remote.exchange(); await f.remote.queue;
  assert.deepEqual(f.commands, ['capture']);
  assert.equal(f.reports[2].ack, 1);
});

test('acknowledging an old report does not consume a replacement source result', async () => {
  const f = fixture();
  f.player.results = [{name:'old'}];
  f.failure(true); await assert.rejects(f.remote.exchange());
  f.player.results = [{name:'new'}];
  f.failure(false); await f.remote.exchange();
  assert.deepEqual(f.player.results, [{name:'new'}]);
  f.response({active:false,commands:[]});
  await f.remote.exchange();
  assert.equal(f.player.passive, true);
  assert.deepEqual(f.commands, ['stop']);
});
