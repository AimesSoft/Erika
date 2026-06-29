# Third-Party Notices

Erika's own source code is licensed under **MPL-2.0** (see `LICENSE`).

The prebuilt `erika_capi` binaries in this bundle **statically link** the
following third-party native libraries, built with Erika's `lgpl` dependency
profile. Their licenses apply to the corresponding portions of the binary.

| Component | Version | License |
|-----------|---------|---------|
| FFmpeg (libav*) | 7.x | LGPL v3 (configured `--disable-gpl --enable-version3`) |
| libass | 0.17.x | ISC |
| FreeType | 2.13.x | FTL / GPLv2 (FTL used here) |
| HarfBuzz | bundled | MIT (Old) |
| FriBidi | 1.0.x | LGPL v2.1+ |
| zlib | 1.3.x | zlib |

## LGPL compliance (FFmpeg, FriBidi)

These binaries link LGPL components statically. To honor the LGPL's relinking
requirement, the complete corresponding source and the reproducible build
system that produced these binaries are publicly available:

- **Erika source** (this exact build): the Git tag named in the release / the
  commit recorded in the bundle's `MANIFEST.txt`, at
  <https://github.com/AimesSoft/Erika>.
- **Native dependency build**: `xtask deps build --all --profile lgpl` plus the
  per-target build described in [`docs/building.md`](../docs/building.md).

Anyone who receives these binaries can therefore rebuild `erika_capi` against a
modified version of FFmpeg (or any other LGPL component above) by checking out
that source and re-running the build with their replacement library.

Full upstream license texts are distributed with each library's source archive
(retrievable via `xtask deps fetch`) and at each project's homepage.
