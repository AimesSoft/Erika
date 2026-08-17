import { ErikaPlayer } from 'erika';

class ErikaSurfaceController extends XComponentController {
  private readonly player: ErikaPlayer = new ErikaPlayer();
  private attached: boolean = false;

  onSurfaceChanged(surfaceId: string, rect: SurfaceRect): void {
    if (!this.attached) {
      this.player.attachSurface({
        surfaceId: BigInt(surfaceId),
        width: rect.surfaceWidth,
        height: rect.surfaceHeight,
        scale: 1.0,
      });
      this.attached = true;
      return;
    }
    this.player.resizeSurface(rect.surfaceWidth, rect.surfaceHeight);
  }

  onSurfaceDestroyed(_surfaceId: string): void {
    if (this.attached) {
      this.player.detachSurface();
      this.attached = false;
    }
    this.player.dispose();
  }
}

@Entry
@Component
struct Index {
  surfaceController: XComponentController = new ErikaSurfaceController();

  build() {
    Column() {
      XComponent({
        type: XComponentType.SURFACE,
        controller: this.surfaceController,
      })
        .width('100%')
        .height('100%')
        .onLoad(() => {
          const surfaceId: string = this.surfaceController.getXComponentSurfaceId();
          console.info(`Erika XComponent surface ready: ${surfaceId}`);
        })
    }
      .width('100%')
      .height('100%')
  }
}
