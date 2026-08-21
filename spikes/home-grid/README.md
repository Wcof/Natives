# HOME-P0 Grid Spike

Isolated React 19 + Next 15 fixture for `react-grid-layout` v2. This app is not
imported by any production route.

```sh
rtk npm run spike:home-grid:test
rtk npm run spike:home-grid:typecheck
rtk npm run spike:home-grid:build
rtk npm run spike:home-grid
```

Open `http://127.0.0.1:3107`. The fixture selector covers 5, 20, and 40 items.
Drag and resize are disabled until Edit is enabled. The displayed commit count
is updated only by `onDragStop` and `onResizeStop`; no move callback is wired.

This spike does not provide packaged Tauri/WebKit, Retina hardware, long-run
RSS, or 100/200-cycle headed evidence. Those remain part of H0-014 and the
release gate before production adoption.
