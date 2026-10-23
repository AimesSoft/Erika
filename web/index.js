import { BrowserPlayer } from './player.js';
import { AudioSession } from './liberika/index.js';
import { RemotePlayback } from './remote.js';

/** Bern extension. Erika's Rust engine owns business state, either in WASM or
 * in the native WebUI host. This adapter owns browser resources and decoding. */
export default function setup(api) {
  // Register before the file picker: the WASM/Promise command boundary loses
  // Safari's transient user gesture by the time the Player is created.
  let audioSession = new AudioSession();
  // Keep app-specific video overlay contrast out of Bern's normal controls.
  const stylesheet = document.createElement('link');
  stylesheet.rel = 'stylesheet';
  const stylesheetURL = new URL('./player.css', import.meta.url);
  stylesheetURL.search = new URL(import.meta.url).search;
  stylesheet.href = stylesheetURL.href;
  document.head.append(stylesheet);
  const player = () => api.media.player('video');
  const encode = (value, out, capacity) => api.write(out, capacity, new TextEncoder().encode(JSON.stringify(value)));
  const execute = async (message, current = () => true) => {
    const p = player();
    if (message.op === 'open') {
      p.startAt = message.start;
      await api.media.command('open', 'video', message.url);
      if (current()) await api.media.command('play', 'video');
    } else if (['play', 'pause', 'seek', 'set_volume'].includes(message.op)) {
      await api.media.command(message.op, 'video', message.value);
    } else if (message.op === 'close') await api.media.stop('video');
    else p.setting(message);
  };
  const remote = api.mode === 'ui' ? new RemotePlayback(api, player, execute) : null;
  return {
    start: () => remote?.start(),
    file: (url, file) => remote?.file(url, file),
    media: {
      name: 'liberika-webcodecs',
      create: (id, report) => {
        if (audioSession.context.state === 'closed') audioSession = new AudioSession();
        const p = new BrowserPlayer(id, report, api.pageFiles, audioSession);
        p.passive = !!remote;
        return p;
      },
    },
    imports: {
      erika_web: {
        ready: () => 1,
        request: (ptr, len) => {
          const message = JSON.parse(api.text(ptr, len));
          api.defer(() => {
            execute(message).catch(error => player().fail(error));
          });
        },
        snapshot: (out, capacity) => encode(player().snapshot(), out, capacity),
        operation_result: (out, capacity) => {
          const p = player(), value = p.operations[0] ?? null;
          const length = encode(value, out, capacity);
          if (length <= capacity && value !== null) p.operations.shift();
          return length;
        },
        // Peek on the sizing call, consume only after copying successfully.
        result: (out, capacity) => {
          const p = player(), value = p.results[0] ?? null;
          const length = encode(value, out, capacity);
          if (length <= capacity && value !== null) p.results.shift();
          return length;
        },
      },
    },
  };
}
