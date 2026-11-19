// getRandomValues, unlike randomUUID, is also available on LAN HTTP pages.
function clientId() {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** Browser executor for the native WebUI engine. Retries exchange observations,
 * never user commands; acknowledged command IDs are scheduled exactly once. */
export class RemotePlayback {
  constructor(api, player, execute) {
    this.api = api;
    this.player = player;
    this.execute = execute;
    this.client = clientId();
    this.ack = 0;
    this.report = 0;
    this.generation = 0;
    this.active = false;
    this.queue = Promise.resolve();
  }
  file(url, file) {
    return this.api.remote.call('erika.browser_file', { client: this.client, url, name: file.name });
  }
  async start() {
    await this.exchange();
    const poll = async () => {
      try { await this.exchange(); }
      catch {
        // Keep local Files for reconnect, but don't play without the host.
        await this.player().pause().catch(() => {});
      }
      this.timer = setTimeout(poll, 100);
    };
    this.timer = setTimeout(poll, 100);
    addEventListener('pagehide', () => clearTimeout(this.timer), { once: true });
  }
  async exchange() {
    const p = this.player();
    // Keep the identical report until it is acknowledged. The host deduplicates
    // its result batches if the connection drops after accepting the request.
    this.packet ??= {
      ack: this.ack, report: ++this.report, generation: this.generation,
      snapshot: p.snapshot(), results: p.results.slice(), operations: p.operations.slice(),
    };
    const response = JSON.parse(await this.api.remote.call('erika.browser_exchange', {
      client: this.client, packet: JSON.stringify(this.packet),
    }));
    for (const key of ['results', 'operations']) {
      for (const item of this.packet[key]) {
        const index = p[key].indexOf(item);
        if (index >= 0) p[key].splice(index, 1);
      }
    }
    this.packet = null;
    p.passive = !response.active;
    if (!response.active) {
      if (this.active) await this.api.media.stop('video');
      this.active = false;
      return;
    }
    this.active = true;
    this.generation = response.generation;
    for (const { id, message } of response.commands) {
      if (id <= this.ack) continue;
      this.ack = id;
      const generation = this.generation;
      const current = () => this.active && this.generation === generation;
      this.queue = this.queue.then(async () => {
        if (!current()) return;
        try { await this.execute(message, current); }
        catch (error) { if (current()) p.fail(error); }
      });
    }
  }
}
