# erika

Native ArkTS/HarmonyOS SDK powered by Erika. This package is independent of
Flutter and exposes the Erika presenter through an ArkTS-friendly API.

The first package target is OpenHarmony arm64. The package contains the native
N-API bridge and the matching `liberika_capi.so` runtime. A host application
provides an `XComponent` surface id through `attachSurface()` and drives
`renderTick()` from its frame scheduler.

```ts
import { ErikaPlayer } from 'erika';

const player = new ErikaPlayer();
const surfaceId = BigInt(xComponentController.getXComponentSurfaceId());
player.attachSurface({ surfaceId, width, height, scale: 1.0 });
player.open('https://example.com/video.mp4');
player.play();
```

The package is licensed under MPL-2.0. See `THIRD_PARTY_NOTICES.md` for the
licenses of the bundled native dependencies.
