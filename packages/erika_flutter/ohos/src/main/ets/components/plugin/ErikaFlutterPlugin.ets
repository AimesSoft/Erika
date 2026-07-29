import {
  EventChannel,
  EventSink,
  FlutterPlugin,
  FlutterPluginBinding,
  MethodCall,
  MethodCallHandler,
  MethodChannel,
  MethodResult,
  StandardMethodCodec,
  SurfaceTextureEntry,
} from '@ohos/flutter_ohos';
import erikaNative from 'liberika_flutter.so';

interface NativeResponse {
  ok: boolean;
  status: number;
  error?: string;
  value?: ESObject;
}

interface ErikaTexture {
  entry: SurfaceTextureEntry;
  surfaceId: number;
  width: number;
  height: number;
  scale: number;
  playerId: number;
}

const PLAYER_CHANNEL: string = 'erika_flutter/player';
const EVENT_CHANNEL: string = 'erika_flutter/events';
const FRAME_INTERVAL_MS: number = 16;

export default class ErikaFlutterPlugin implements FlutterPlugin, MethodCallHandler {
  private binding: FlutterPluginBinding | null = null;
  private methodChannel: MethodChannel | null = null;
  private eventChannel: EventChannel | null = null;
  private eventSink: EventSink | null = null;
  private players: Set<number> = new Set();
  private textures: Map<number, ErikaTexture> = new Map();
  private frameTimer: number = -1;

  getUniqueClassName(): string {
    return 'ErikaFlutterPlugin';
  }

  onAttachedToEngine(binding: FlutterPluginBinding): void {
    this.binding = binding;
    const messenger = binding.getBinaryMessenger();
    this.methodChannel = new MethodChannel(
      messenger,
      PLAYER_CHANNEL,
      StandardMethodCodec.INSTANCE,
    );
    this.methodChannel.setMethodCallHandler(this);
    this.eventChannel = new EventChannel(
      messenger,
      EVENT_CHANNEL,
      StandardMethodCodec.INSTANCE,
    );
    this.eventChannel.setStreamHandler({
      onListen: (_args: ESObject, sink: EventSink): void => {
        this.eventSink = sink;
        this.drainAllEvents();
      },
      onCancel: (_args: ESObject): void => {
        this.eventSink = null;
      },
    });
    this.startFrameLoop();
  }

  onDetachedFromEngine(_binding: FlutterPluginBinding): void {
    this.stopFrameLoop();
    this.textures.forEach((texture: ErikaTexture, textureId: number): void => {
      if (texture.playerId > 0) {
        erikaNative.nativeDetachSurface(texture.playerId);
      }
      this.binding?.getTextureRegistry()?.unregisterTexture(textureId);
    });
    this.textures.clear();
    this.players.forEach((playerId: number): void => {
      erikaNative.nativeDestroy(playerId);
    });
    this.players.clear();
    this.methodChannel?.setMethodCallHandler(null);
    this.eventChannel?.setStreamHandler(null);
    this.methodChannel = null;
    this.eventChannel = null;
    this.eventSink = null;
    this.binding = null;
  }

  onMethodCall(call: MethodCall, result: MethodResult): void {
    const args: Map<string, ESObject> = call.args as Map<string, ESObject>;
    if (call.method === 'create') {
      this.createPlayer(args, result);
      return;
    }
    if (call.method === 'dispose') {
      this.disposePlayer(args, result);
      return;
    }
    if (call.method === 'createTexture') {
      this.createTexture(args, result);
      return;
    }
    if (call.method === 'resizeTexture') {
      this.resizeTexture(args, result);
      return;
    }
    if (call.method === 'releaseTexture') {
      this.releaseTexture(args, result);
      return;
    }
    if (call.method === 'attachView') {
      this.attachView(args, result);
      return;
    }
    if (call.method === 'detachView') {
      this.detachView(args, result);
      return;
    }
    if (call.method === 'screenshot') {
      result.success(null);
      return;
    }
    if (this.isNativePlayerMethod(call.method)) {
      this.invokePlayer(call.method, args, result);
      return;
    }
    result.notImplemented();
  }

  private createPlayer(args: Map<string, ESObject>, result: MethodResult): void {
    const outputMode = this.numberArg(args, 'outputMode', 0);
    const headroom = this.numberArg(
      args,
      'edrHeadroom',
      outputMode === 2 ? 4.0 : 1.0,
    );
    const upscaler = this.numberArg(args, 'upscaler', 0);
    const playerId = erikaNative.nativeCreate(outputMode, headroom, upscaler) as number;
    if (playerId <= 0) {
      result.error(
        'ERIKA_ERROR',
        'Erika OpenHarmony presenter creation failed: ' +
          (erikaNative.nativeLastError() as string),
        null,
      );
      return;
    }
    this.players.add(playerId);
    result.success(playerId);
  }

  private disposePlayer(args: Map<string, ESObject>, result: MethodResult): void {
    const playerId = this.numberArg(args, 'playerId', 0);
    this.textures.forEach((texture: ErikaTexture): void => {
      if (texture.playerId === playerId) {
        erikaNative.nativeDetachSurface(playerId);
        texture.playerId = 0;
      }
    });
    if (this.players.delete(playerId)) {
      erikaNative.nativeDestroy(playerId);
    }
    result.success(null);
  }

  private createTexture(args: Map<string, ESObject>, result: MethodResult): void {
    const registry = this.binding?.getTextureRegistry();
    if (!registry) {
      result.error('NO_TEXTURE_REGISTRY', 'TextureRegistry is unavailable', null);
      return;
    }
    const width = Math.max(1, Math.trunc(this.numberArg(args, 'width', 1)));
    const height = Math.max(1, Math.trunc(this.numberArg(args, 'height', 1)));
    const scale = Math.max(0.1, this.numberArg(args, 'scale', 1.0));
    const textureId = registry.getTextureId();
    const entry = registry.registerTexture(textureId);
    registry.setTextureBufferSize(textureId, width, height);
    this.textures.set(textureId, {
      entry: entry,
      surfaceId: entry.getSurfaceId(),
      width: width,
      height: height,
      scale: scale,
      playerId: 0,
    });
    result.success(textureId);
  }

  private resizeTexture(args: Map<string, ESObject>, result: MethodResult): void {
    const textureId = this.numberArg(args, 'textureId', -1);
    const texture = this.textures.get(textureId);
    if (!texture) {
      result.error('ERIKA_ERROR', 'Unknown Erika texture ' + textureId, null);
      return;
    }
    texture.width = Math.max(1, Math.trunc(this.numberArg(args, 'width', texture.width)));
    texture.height = Math.max(1, Math.trunc(this.numberArg(args, 'height', texture.height)));
    texture.scale = Math.max(0.1, this.numberArg(args, 'scale', texture.scale));
    this.binding?.getTextureRegistry()?.setTextureBufferSize(
      textureId,
      texture.width,
      texture.height,
    );
    if (texture.playerId > 0) {
      const status = erikaNative.nativeResizeSurface(
        texture.playerId,
        texture.width,
        texture.height,
        texture.scale,
      ) as number;
      if (!this.completeStatus(status, result, 'resize OpenHarmony surface')) {
        return;
      }
    }
    result.success(null);
  }

  private releaseTexture(args: Map<string, ESObject>, result: MethodResult): void {
    const textureId = this.numberArg(args, 'textureId', -1);
    const texture = this.textures.get(textureId);
    if (texture) {
      if (texture.playerId > 0) {
        erikaNative.nativeDetachSurface(texture.playerId);
      }
      this.binding?.getTextureRegistry()?.unregisterTexture(textureId);
      this.textures.delete(textureId);
    }
    result.success(null);
  }

  private attachView(args: Map<string, ESObject>, result: MethodResult): void {
    const playerId = this.numberArg(args, 'playerId', 0);
    const textureId = this.numberArg(args, 'viewId', -1);
    const texture = this.textures.get(textureId);
    if (!this.players.has(playerId) || !texture) {
      result.error('ERIKA_ERROR', 'Unknown Erika player or texture', null);
      return;
    }
    if (texture.playerId > 0 && texture.playerId !== playerId) {
      erikaNative.nativeDetachSurface(texture.playerId);
    }
    const status = erikaNative.nativeAttachSurface(
      playerId,
      texture.surfaceId,
      texture.width,
      texture.height,
      texture.scale,
    ) as number;
    if (!this.completeStatus(status, result, 'attach OpenHarmony surface')) {
      return;
    }
    texture.playerId = playerId;
    result.success(null);
    this.drainEvents(playerId);
  }

  private detachView(args: Map<string, ESObject>, result: MethodResult): void {
    const playerId = this.numberArg(args, 'playerId', 0);
    const textureId = this.numberArg(args, 'viewId', -1);
    const texture = this.textures.get(textureId);
    if (!texture || texture.playerId !== playerId) {
      result.success(null);
      return;
    }
    const status = erikaNative.nativeDetachSurface(playerId) as number;
    texture.playerId = 0;
    if (!this.completeStatus(status, result, 'detach OpenHarmony surface')) {
      return;
    }
    result.success(null);
    this.drainEvents(playerId);
  }

  private invokePlayer(
    method: string,
    args: Map<string, ESObject>,
    result: MethodResult,
  ): void {
    const playerId = this.numberArg(args, 'playerId', 0);
    if (!this.players.has(playerId)) {
      result.error('ERIKA_ERROR', 'Unknown Erika player ' + playerId, null);
      return;
    }
    const argumentsJson = JSON.stringify(this.mapToObject(args));
    const raw = erikaNative.nativeInvoke(playerId, method, argumentsJson) as string;
    const response = JSON.parse(raw) as NativeResponse;
    if (!response.ok) {
      result.error(
        'ERIKA_ERROR',
        response.error ?? ('Erika native method failed with status ' + response.status),
        response,
      );
      return;
    }
    result.success(response.value ?? null);
    this.drainEvents(playerId);
  }

  private startFrameLoop(): void {
    if (this.frameTimer >= 0) {
      return;
    }
    this.frameTimer = setInterval((): void => {
      const frameTimeSeconds = Date.now() / 1000.0;
      this.players.forEach((playerId: number): void => {
        erikaNative.nativeRenderTick(playerId, frameTimeSeconds);
        this.drainEvents(playerId);
      });
    }, FRAME_INTERVAL_MS);
  }

  private stopFrameLoop(): void {
    if (this.frameTimer >= 0) {
      clearInterval(this.frameTimer);
      this.frameTimer = -1;
    }
  }

  private drainAllEvents(): void {
    this.players.forEach((playerId: number): void => this.drainEvents(playerId));
  }

  private drainEvents(playerId: number): void {
    const sink = this.eventSink;
    if (!sink) {
      return;
    }
    for (let index = 0; index < 64; index += 1) {
      const raw = erikaNative.nativePollEvent(playerId) as string | null;
      if (raw === null) {
        return;
      }
      const response = JSON.parse(raw) as NativeResponse;
      if (!response.ok || response.value === undefined) {
        return;
      }
      const event: ESObject = response.value;
      event.playerId = playerId;
      sink.success(event);
    }
  }

  private completeStatus(status: number, result: MethodResult, operation: string): boolean {
    if (status === 0) {
      return true;
    }
    const nativeError = erikaNative.nativeLastError() as string;
    result.error(
      'ERIKA_ERROR',
      operation + ' failed' + (nativeError.length > 0 ? ': ' + nativeError : ''),
      status,
    );
    return false;
  }

  private numberArg(
    args: Map<string, ESObject>,
    name: string,
    fallback: number,
  ): number {
    const value: ESObject = args.get(name);
    return typeof value === 'number' ? value as number : fallback;
  }

  private mapToObject(args: Map<string, ESObject>): ESObject {
    const object: ESObject = {};
    args.forEach((value: ESObject, key: string): void => {
      object[key] = this.jsonValue(value);
    });
    return object;
  }

  private jsonValue(value: ESObject): ESObject {
    if (value instanceof Map) {
      return this.mapToObject(value as Map<string, ESObject>);
    }
    if (Array.isArray(value)) {
      return (value as Array<ESObject>).map(
        (child: ESObject): ESObject => this.jsonValue(child),
      );
    }
    return value;
  }

  private isNativePlayerMethod(method: string): boolean {
    return [
      'open', 'play', 'pause', 'stop', 'close', 'seek',
      'setPlaybackRate', 'setVolume', 'setUpscaler', 'setSubtitleScale',
      'getUpscalerStatus', 'getOutputStatus', 'getPresenterStats',
      'addExternalSubtitle', 'removeSubtitleTrack',
      'loadDanmakuFile', 'loadDanmakuJson',
      'addDanmakuTrackFile', 'addDanmakuTrackJson',
      'removeDanmakuTrack', 'setDanmakuTrackEnabled',
      'setDanmakuTrackOffset', 'setDanmakuGlobalOffset',
      'danmakuTracks', 'clearDanmaku', 'setDanmakuEnabled',
      'setDanmakuConfig', 'selectAudioTrack', 'selectSubtitleTrack', 'tracks',
    ].includes(method);
  }
}
