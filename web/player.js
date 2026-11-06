import { Player, AudioSession } from './liberika/index.js';

const nameTrack = (track) => ({ id: track.id, name: [track.name || track.language, track.codec?.toUpperCase()].filter(Boolean).join(' · ') || `Track ${track.id}` });
const MAX_TEXT = 64 * 1024 * 1024;

/** One persistent surface per Bern player. View/menu changes move this same
 * canvas, so they neither restart playback nor discard the text atlases. */
export class BrowserPlayer {
  constructor(id, report, files, audioSession) {
    this.audioSession = audioSession;
    this.id = id;
    this.reportToHub = report;
    this.files = files;
    this.version = 0n;
    this.epoch = 0;
    this.queue = Promise.resolve();
    this.source = null;
    this.instance = null;
    this.info = null;
    this.error = null;
    this.ended = false;
    this.volume = 0.7;
    this.muted = false;
    this.rate = 1;
    this.hardware = true;
    this.startAt = 0;
    this.danmaku = {};
    this.subtitles = {};
    this.external = new Map();
    this.results = [];
    this.operations = [];
    this.video = document.createElement('div');
    this.video.className = 'bern-video-element';
    this.video.setAttribute('aria-hidden', 'true');
    this.canvas = document.createElement('canvas');
    this.canvas.style.cssText = 'width:100%;height:100%;display:block';
    this.video.append(this.canvas);
    this.lastReport = 0;
  }
  task(fn) {
    const epoch = this.epoch;
    const result = this.queue.then(async () => {
      if (epoch !== this.epoch) return;
      try { return await fn(); }
      catch (error) { if (epoch === this.epoch) throw error; }
    });
    this.queue = result.catch(() => {});
    return result;
  }
  async ensure() {
    if (this.instance) return this.instance;
    if (this.audioSession?.context.state === 'closed') this.audioSession = new AudioSession();
    const p = await Player.create({ canvas: this.canvas, volume: this.muted ? 0 : this.volume, audioSession: this.audioSession });
    this.instance = p;
    for (const event of ['play', 'pause', 'stats']) p.addEventListener(event, () => this.report());
    p.addEventListener('timeupdate', () => {
      if (performance.now() - this.lastReport >= 250) this.report();
    });
    p.addEventListener('ended', () => { this.ended = true; this.report(); });
    p.addEventListener('error', event => { if (this.info) this.fail(event.detail); });
    return p;
  }
  load(url, version) {
    this.version = version;
    this.epoch++;
    const epoch = this.epoch, start = this.startAt;
    this.startAt = 0;
    this.source = url;
    this.info = null;
    this.error = null;
    this.ended = false;
    this.results = [];
    this.pendingSettings?.clear();
    this.external.clear();
    this.subtitleSelected = null;
    this.clearStill();
    this.report();
    return this.task(async () => {
      const p = await this.ensure();
      const info = await p.open(this.files.get(url) ?? served(url), { hardwareDecode: this.hardware });
      if (epoch !== this.epoch) return;
      this.info = info;
      this.subtitleSelected = info.subtitleTrack;
      p.setVolume(this.muted ? 0 : this.volume);
      await p.configureDanmaku(this.danmaku);
      await p.configureSubtitles(this.subtitles);
      if (p.rate !== this.rate) await p.setRate(this.rate);
      if (start > 0) await p.seek(Math.min(start, info.duration || start));
      this.report();
    }).catch(error => { if (epoch === this.epoch) this.fail(error); throw error; });
  }
  unload(version) {
    this.version = version;
    this.epoch++;
    this.source = null;
    this.info = null;
    this.error = null;
    this.results = [];
    this.pendingSettings?.clear();
    this.external.clear();
    this.clearStill();
    this.report();
    return this.task(() => this.instance?.close());
  }
  play() {
    return this.task(async () => {
      if (!this.info || this.error) throw new Error(this.error || 'Open media before playing');
      if (this.ended) { this.ended = false; await this.instance.seek(0); }
      await this.instance.play();
      this.report();
    });
  }
  pause() { return this.task(async () => { await this.instance?.pause(); this.report(); }); }
  seek(seconds) {
    return this.task(async () => {
      if (!this.info) return;
      this.ended = false;
      await this.instance.seek(seconds);
      this.report();
    });
  }
  setVolume(value) {
    this.volume = Math.max(0, Math.min(1, value));
    this.instance?.setVolume(this.muted ? 0 : this.volume);
    this.report();
  }
  setMuted(value) { this.muted = value; this.setVolume(this.volume); }
  fail(error) {
    this.error = String(error?.message ?? error);
    this.instance?.pause().catch(() => {});
    this.report();
  }
  report() {
    if (this.passive) return;
    this.lastReport = performance.now();
    const p = this.instance;
    this.reportToHub(this.version, {
      phase: !this.source ? 'Idle' : this.error ? 'Failed' : !this.info ? 'Loading' : this.ended ? 'Ended' : p?.playing ? 'Playing' : 'Paused',
      position: this.info ? p?.currentTime || 0 : 0,
      duration: this.info?.duration || null,
      seekable: !!this.info?.duration,
      volume: this.volume, muted: this.muted, error: this.error,
    });
  }
  snapshot() {
    const p = this.instance, info = this.info;
    const video = info?.tracks.find(t => t.id === info.videoTrack);
    return {
      source: this.source, loaded: !!info && !this.error,
      playing: !!info && !!p?.playing, ended: this.ended, picture: !!info && !!p?.stats.hasVideo,
      position: info ? p?.currentTime || 0 : 0, duration: info?.duration || null,
      audioTracks: info?.tracks.filter(t => t.type === 'audio').map(nameTrack) ?? [],
      subtitleTracks: [...(info?.tracks.filter(t => t.type === 'subtitle' && ['ass', 'ssa'].includes(t.codec)).map(nameTrack) ?? []), ...Array.from(this.external, ([id, item]) => ({ id, name: item.name }))],
      audioTrack: info ? p?.info?.audioTrack : null, subtitleTrack: this.subtitleSelected,
      chapters: info?.chapters ?? [], video: video ? `${video.codec.toUpperCase()} · ${video.height}p · WebCodecs` : '',
      still: this.still ?? null, error: this.error,
    };
  }
  async text(url) {
    const file = this.files.get(url);
    if (file) {
      if (file.size > MAX_TEXT) throw new Error('Text file exceeds 64 MiB');
      return file.text();
    }
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Cannot read text: HTTP ${response.status}`);
    const reader = response.body.getReader(), decoder = new TextDecoder();
    let size = 0, text = '';
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        size += value.length;
        if (size > MAX_TEXT) throw new Error('Text file exceeds 64 MiB');
        text += decoder.decode(value, { stream: true });
      }
      return text + decoder.decode();
    } finally { await reader.cancel(); }
  }
  setting(message) {
    const { op, value, config } = message;
    if (op === 'hardware') { this.hardware = value; return; }
    if (op === 'rate') this.rate = value;
    if (op === 'danmakuOptions') Object.assign(this.danmaku, config);
    if (op === 'subtitleOptions') Object.assign(this.subtitles, config);
    if (['rate', 'danmakuOptions', 'subtitleOptions'].includes(op)) {
      // At most one pending update per setting type; use the latest slider value.
      this.pendingSettings ??= new Set();
      if (!this.info || this.pendingSettings.has(op)) return;
      this.pendingSettings.add(op);
      this.task(async () => {
        this.pendingSettings.delete(op);
        if (!this.info) return;
        if (op === 'rate') await this.instance.setRate(this.rate);
        else if (op === 'danmakuOptions') await this.instance.configureDanmaku(this.danmaku);
        else await this.instance.configureSubtitles(this.subtitles);
      }).catch(error => this.operations.push({ source: this.source, op: "setting", error: String(error) }));
      return;
    }
    const epoch = this.epoch;
    this.task(async () => {
      if (!this.info) return;
      const p = this.instance;
      if (op === 'audio') await p.selectAudio(value);
      else if (op === 'subtitle') {
        const external = this.external.get(value);
        if (external) await p.addSubtitle(external.text);
        else await p.selectSubtitle(value);
        this.subtitleSelected = value;
      } else if (op === 'addSubtitle') {
        const text = await this.text(message.url);
        if (epoch !== this.epoch) return;
        await p.addSubtitle(text);
        const id = 1000000 + this.external.size;
        const name = this.files.get(message.url)?.name ?? decodeURIComponent(new URL(message.url).pathname.split('/').at(-1));
        this.external.set(id, { name, text });
        this.subtitleSelected = id;
        this.operations.push({ source: this.source, op: "subtitle", url: message.url });
      } else if (op === 'addDanmaku') {
        try {
          const text = message.text ?? await this.text(message.url);
          if (epoch !== this.epoch) return;
          const count = await p.addDanmaku(text);
          if (epoch === this.epoch && message.notify) this.results.push({ name: message.name, count });
        } catch (error) {
          if (epoch === this.epoch) this.results.push({ name: message.name, error: String(error) });
        }
      } else if (op === 'sendDanmaku') await p.sendDanmaku(message.text, message.color);
      else if (op === 'capture') {
        const blob = await p.capture(message.width);
        if (epoch !== this.epoch) return;
        const url = URL.createObjectURL(blob);
        if (message.width) { this.clearStill(); this.still = url; }
        else {
          const a = document.createElement('a');
          a.href = url; a.download = message.name || 'Erika.png'; a.click();
          setTimeout(() => URL.revokeObjectURL(url), 60000);
          this.operations.push({ source: this.source, op: 'screenshot', name: message.name || 'Erika.png' });
        }
      }
      this.report();
    }).catch(error => {
      if (epoch === this.epoch) this.operations.push({ source: this.source, op: 'setting', error: String(error?.message ?? error) });
    });
  }
  clearStill() {
    if (this.still) {
      const previous = this.still;
      // Bern may still be decoding the departing image for its hero transition.
      // Let that view finish before revoking its temporary capture URL.
      setTimeout(() => URL.revokeObjectURL(previous), 2000);
    }
    this.still = null;
  }
  async release() {
    this.epoch++;
    await this.queue;
    await this.instance?.destroy();
    this.instance = null;
    this.clearStill();
    this.video.remove();
  }
}

/** A file of the device the engine runs on, by remote access on the port that
 * served this page; other addresses as they are. */
function served(url) {
  if (!url.startsWith('file://')) return url;
  return new URL(`/api/media/local/manage/stream?path=${url.slice('file://'.length)}`, location.href).href;
}
