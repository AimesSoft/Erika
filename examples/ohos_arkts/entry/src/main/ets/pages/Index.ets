import { ErikaNativeResponse, ErikaPlayer } from 'erika';

class ErikaSurfaceController extends XComponentController {
  private readonly player: ErikaPlayer = new ErikaPlayer();
  private attached: boolean = false;
  private pendingUri: string = '';

  onSurfaceCreated(surfaceId: string): void {
    console.info(`Erika XComponent surface created: ${surfaceId}`);
  }

  onSurfaceChanged(surfaceId: string, rect: SurfaceRect): void {
    if (!this.attached) {
      this.player.attachSurface({
        surfaceId: BigInt(surfaceId),
        width: rect.surfaceWidth,
        height: rect.surfaceHeight,
        scale: 1.0,
      });
      this.attached = true;
    } else {
      this.player.resizeSurface(rect.surfaceWidth, rect.surfaceHeight);
    }
    this.startPendingUri();
  }

  onSurfaceDestroyed(_surfaceId: string): void {
    if (this.attached) {
      this.player.detachSurface();
      this.attached = false;
    }
    this.player.dispose();
  }

  open(uri: string): void {
    this.pendingUri = uri;
    this.startPendingUri();
  }

  renderFrame(timeSeconds: number): ErikaNativeResponse {
    return this.player.renderTick(timeSeconds);
  }

  private startPendingUri(): void {
    if (!this.attached || this.pendingUri.length === 0) {
      return;
    }
    const uri: string = this.pendingUri;
    this.pendingUri = '';
    this.player.open(uri);
    this.player.play();
  }
}

@Entry
@Component
struct Index {
  surfaceController: ErikaSurfaceController = new ErikaSurfaceController();

  build() {
    Column() {
      XComponent({
        id: 'erika-surface',
        type: XComponentType.SURFACE,
        controller: this.surfaceController,
      })
        .width('100%')
        .height('100%')
        .onLoad(() => {
          this.surfaceController.open('https://example.com/video.mp4');
        })
    }
      .width('100%')
      .height('100%')
  }
}
