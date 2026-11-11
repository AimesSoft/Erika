// Erika's side of a plugin's context. `__erikaConfig` comes first, then this,
// then liberika's prelude (`player`, `danmaku`), then the plugin. The engine
// cannot be called from here: before each call it hands over what the plugin
// may read (`__erika.snapshot`), and afterwards it reads back the calls the
// plugin queued. Every function the engine calls returns
// `{ value, error, calls, entries, blockWords }`.
var __erika = {
  config: __erikaConfig,
  snapshot: {},
  outbox: [],
  waiting: {},
  next: 1,
};

var __host = {
  has: function (permission) {
    return __erika.config.permissions.indexOf(permission) >= 0;
  },
  read: function (name) {
    return __erika.snapshot[name];
  },
  call: function (name, args) {
    __erika.outbox.push([name, args || []]);
  },
};

var plugin = {
  id: __erika.config.id,
  name: __erika.config.name,
  version: __erika.config.version,
  permissions: __erika.config.permissions.slice(),
  hasPermission: function (permission) {
    return __host.has(permission);
  },
};

var ui = {
  showSnackBar: function (message) {
    if (!__host.has('ui.dialog')) return;
    __host.call('uiShowToast', [String(message)]);
  },
  showDialog: function (title, content) {
    if (!__host.has('ui.dialog')) return false;
    __host.call('uiShowDialog', [String(title), content == null ? '' : String(content)]);
    return true;
  },
  showLoading: function (message) {
    if (!__host.has('ui.dialog')) return;
    __host.call('uiShowToast', [String(message)]);
  },
  hideLoading: function () {},
};

function __erikaPart(name) {
  return (__erika.snapshot[name] = __erika.snapshot[name] || {});
}

var storage = {
  set: function (key, value) {
    if (!__host.has('storage')) return false;
    var text = JSON.stringify(value === undefined ? null : value);
    __erikaPart('storage')[String(key)] = text;
    __host.call('storageSet', [String(key), text]);
    return true;
  },
  get: function (key) {
    if (!__host.has('storage')) return null;
    var text = __erikaPart('storage')[String(key)];
    if (text == null) return null;
    try {
      return JSON.parse(text);
    } catch (error) {
      return text;
    }
  },
  remove: function (key) {
    if (!__host.has('storage')) return false;
    delete __erikaPart('storage')[String(key)];
    __host.call('storageRemove', [String(key)]);
    return true;
  },
  clear: function () {
    if (!__host.has('storage')) return false;
    __erika.snapshot.storage = {};
    __host.call('storageClear', []);
    return true;
  },
};

// NipaPlay's, with an optional value for what the user never set.
var settings = {
  getText: function (entry, fallback) {
    var value = __erikaPart('text')[String(entry)];
    return value == null ? (fallback == null ? '' : String(fallback)) : String(value);
  },
  setText: function (entry, value) {
    __erikaPart('text')[String(entry)] = String(value);
    __host.call('pluginSetTextSetting', [String(entry), String(value)]);
  },
  getSwitch: function (entry, fallback) {
    var value = __erikaPart('switch')[String(entry)];
    return value === undefined ? !!fallback : !!value;
  },
  setSwitch: function (entry, value) {
    __erikaPart('switch')[String(entry)] = !!value;
    __host.call('pluginSetSwitchSetting', [String(entry), !!value]);
  },
};

var dev = {
  log: function (message) {
    __host.call('devLog', [String(message)]);
  },
  logError: function (message) {
    __host.call('devLogError', [String(message)]);
  },
};

var system = {
  setDownloaderEnabled: function (enabled) {
    if (!__host.has('system.override')) return false;
    __host.call('systemSetDownloaderEnabled', [!!enabled]);
    return true;
  },
};

var process = {
  start: function (executable, args, env) {
    if (!__host.has('system.override')) return false;
    __host.call('processStart', [String(executable), args || [], env || {}]);
    return true;
  },
  stop: function () {
    if (!__host.has('system.override')) return false;
    __host.call('processStop', []);
    return true;
  },
  isRunning: function () {
    return !!__erika.snapshot['process.running'];
  },
};

// Erika's own: requests that answer later, as a promise of
// `{ status, ok, headers, body, json(), text() }`. They need `network`.
var net = {
  fetch: function (url, options) {
    if (!__host.has('network')) {
      return Promise.reject(new Error('插件没有声明 network 权限'));
    }
    var id = __erika.next++;
    return new Promise(function (resolve, reject) {
      __erika.waiting[id] = { resolve: resolve, reject: reject, kind: 'network' };
      __host.call('netFetch', [id, String(url), options || {}]);
    });
  },
};

function __erikaCatalogueValue(response) {
  if (!response || !response.ok) {
    var info = response && response.error || { code: 'invalid', message: '媒体库没有返回结果' };
    var error = new Error(info.message);
    error.code = info.code;
    throw error;
  }
  return response.value;
}
function __erikaLibraryRequest(permission, method, input) {
  if (!__host.has(permission)) {
    var error = new Error('插件没有声明 ' + permission + ' 权限');
    error.code = 'permission_denied';
    return Promise.reject(error);
  }
  var id = __erika.next++;
  return new Promise(function (resolve, reject) {
    __erika.waiting[id] = { resolve: resolve, reject: reject, kind: 'library' };
    __host.call('libraryRequest', [id, method, input]);
  });
}

// V2 operations resolve only after the host validates and completes the request.
// The two numeric v1 methods below keep their DDP namespace.
var library = {
  apiVersion: 2,
  capabilities: ['media.read', 'media.context', 'media.watched', 'catalogue.apply', 'catalogue.undo', 'sources.import', 'sources.pages', 'sources.status'],
  registerSource: function (descriptor) { return __erikaLibraryRequest('library.sources', 'registerSource', descriptor); },
  beginScan: function (sourceId) { return __erikaLibraryRequest('library.sources', 'beginScan', {source: sourceId}); },
  upsertItems: function (scanId, items, page) { return __erikaLibraryRequest('library.sources', 'upsertItems', {scan: scanId, items: items, page: page}); },
  finishScan: function (scanId, result) { return __erikaLibraryRequest('library.sources', 'finishScan', {scan: scanId, complete: result.complete, pages: result.pages}); },
  getSourceItems: function (sourceId, options) { options = options || {}; return __erikaLibraryRequest('library.sources', 'getSourceItems', {source: sourceId, cursor: options.cursor, limit: options.limit}); },
  getSourceItem: function (reference) { return __erikaLibraryRequest('library.sources', 'getSourceItem', reference); },
  getSourceStatus: function (reference) { return __erikaLibraryRequest('library.sources', 'getSourceStatus', reference); },
  getMediaContext: function () {
    if (!__host.has('library.read')) return null;
    return __erikaCatalogueValue(__erika.snapshot['library.mediaContext']);
  },
  getMedia: function (query) { return __erikaLibraryRequest('library.read', 'getMedia', query); },
  getFiles: function (mediaId) { return __erikaLibraryRequest('library.read', 'getFiles', mediaId); },
  getChildren: function (mediaId) { return __erikaLibraryRequest('library.read', 'getChildren', mediaId); },
  getWatched: function (mediaId) { return __erikaLibraryRequest('library.read', 'getWatched', mediaId); },
  setWatched: function (decision) { return __erikaLibraryRequest('library.write', 'setWatched', decision); },
  apply: function (edit) { return __erikaLibraryRequest('library.write', 'apply', edit); },
  undo: function (change) { return __erikaLibraryRequest('library.write', 'undo', change); },
  getShows: function () {
    if (!__host.has('library.read')) return [];
    return __erika.snapshot.library || [];
  },
  setEpisodeWatched: function (animeId, episode, watched) {
    if (!__host.has('library.write')) return false;
    __host.call('librarySetEpisodeWatched', [Number(animeId), Number(episode), watched !== false]);
    return true;
  },
};

// Erika's own: more of what its controls offer, while the plugin runs.
var extend = {
  // Rates for the speed menu, such as [0.25, 0.5, …, 5].
  speeds: function (rates) {
    if (!__host.has('player.control')) return false;
    __host.call('extendSpeeds', [(rates || []).map(Number)]);
    return true;
  },
  // Colors for sending danmaku: [{ name: '粉', color: '#FF69B4' }, …].
  danmakuColors: function (colors) {
    if (!__host.has('danmaku.modify')) return false;
    __host.call('extendDanmakuColors', [colors || []]);
    return true;
  },
  // A field for a hexadecimal color where danmaku are sent.
  danmakuHexInput: function (enabled) {
    if (!__host.has('danmaku.modify')) return false;
    __host.call('extendDanmakuHexInput', [enabled !== false]);
    return true;
  },
  // Quick select where downloads are added: the folders used before,
  // with those `pluginDownloadSuggestions({ name })` returns as
  // [{ title, folder }].
  downloadQuickSelect: function (enabled) {
    if (!__host.has('library.read')) return false;
    __host.call('extendDownloadQuickSelect', [enabled !== false]);
    return true;
  },
  // Measure the first `seconds` (10 unless given, at most 60) of each video
  // opened: an `openingProfiled` event brings { url, frames: [{ time, red,
  // green, blue, luma, dark, white, red_share }] } where liberika plays.
  openingProfile: function (enabled, seconds) {
    if (!__host.has('player.control')) return false;
    __host.call('extendOpeningProfile', [enabled !== false, Number(seconds) || 10]);
    return true;
  },
  // A button on each episode of a show's details that marks it watched.
  episodeWatchedButton: function (enabled) {
    if (!__host.has('library.write')) return false;
    __host.call('extendWatchedButton', [enabled !== false]);
    return true;
  },
  // A thin line along the bottom of the picture as long as the part played,
  // in a theme color ('accent', 'red', 'orange' or 'green'); null
  // or false takes it away.
  progressStrip: function (color) {
    if (!__host.has('player.overlay')) return false;
    __host.call('extendProgressStrip', [color === undefined ? 'accent' : color]);
    return true;
  },
  // The danmaku density chart along the bottom of the picture: how many
  // comments each stretch of the video holds, in a theme color as above.
  danmakuDensity: function (color) {
    if (!__host.has('player.overlay')) return false;
    __host.call('extendDanmakuDensity', [color === undefined ? 'accent' : color]);
    return true;
  },
};

function __erikaDrain() {
  var calls = __erika.outbox;
  __erika.outbox = [];
  return calls;
}

function __erikaSee(snapshot) {
  for (var name in snapshot || {}) __erika.snapshot[name] = snapshot[name];
}

// What the engine reads back after a call: the plugin's declared lists as
// they are now, and its queued calls.
function __erikaReply(value, error) {
  var exports = globalThis.__erikaPlugin || {};
  return {
    value: value === undefined ? null : value,
    error: error == null ? null : String(error),
    entries: exports.entries || null,
    blockWords: exports.blockWords || null,
    calls: __erikaDrain(),
  };
}

function __erikaMessage(error) {
  return error && error.message ? error.message : String(error);
}

// Run `body`; a promise it returns answers later, by the `__later` call with
// `tag`.
function __erikaRun(tag, body) {
  try {
    var value = body();
    var pending = false;
    if (value && typeof value.then === 'function') {
      pending = true;
      value.then(
        function (result) {
          __host.call('__later', [tag, result === undefined ? null : result, null]);
        },
        function (error) {
          __host.call('__later', [tag, null, __erikaMessage(error)]);
        }
      );
      value = null;
    }
    var reply = __erikaReply(value, null);
    reply.pending = pending;
    return reply;
  } catch (error) {
    return __erikaReply(null, __erikaMessage(error));
  }
}

// Call the plugin's handler `name` with `arg`.
function __erikaInvoke(name, arg, snapshot, tag) {
  __erikaSee(snapshot);
  return __erikaRun(tag, function () {
    var handler = (globalThis.__erikaPlugin || {})[name];
    if (typeof handler !== 'function') return null;
    return arg === undefined ? handler() : handler(arg);
  });
}

// A request `net.fetch` made answered.
function __erikaSettle(id, response, error, snapshot) {
  __erikaSee(snapshot);
  var waiting = __erika.waiting[id];
  delete __erika.waiting[id];
  if (waiting) {
    if (error != null) {
      waiting.reject(new Error(error));
    } else if (waiting.kind === 'library') {
      try { waiting.resolve(__erikaCatalogueValue(response)); }
      catch (failure) { waiting.reject(failure); }
    } else {
      response.json = function () {
        return JSON.parse(response.body);
      };
      response.text = function () {
        return response.body;
      };
      waiting.resolve(response);
    }
  }
  return __erikaReply(null, null);
}

function __erikaLoad(snapshot) {
  __erikaSee(snapshot);
  var exports = globalThis.__erikaPlugin || {};
  var reply = __erikaRun(0, function () {
    if (typeof exports.onInitialize === 'function') return exports.onInitialize();
    return null;
  });
  reply.manifest = exports.manifest || null;
  return reply;
}
