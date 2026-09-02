export function buildDashboardStyles({ backgroundStyles = '', widgetStyles = '' } = {}) {
  return `
    :host { all: initial; }
    :host([data-widgets-hidden="true"]) .Slot,
    :host(.widgets-hidden) .Slot,
    :host-context(body.space-widgets-hidden) .Slot {
      display: none !important;
    }

    .Widgets {
      color: #ffffff;
      width: 100%;
      height: 100%;
      position: relative;
      overflow: hidden;
      padding: 0;
      text-align: center;
      pointer-events: none;
      user-select: auto;
      font-family: -apple-system, BlinkMacSystemFont, 'PingFang SC', 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
    }

    .Widgets a {
      color: inherit;
    }

    .Widgets input {
      font-family: inherit;
      color: inherit;
      border-color: white;
    }

    .Widgets input::placeholder {
      color: inherit;
      opacity: 0.5;
    }
    .Widgets .button {
      border: 0;
      border-radius: 2em;
      color: var(--text-on-primary, #111);
      cursor: pointer;
      display: inline-block;
      padding: 0.5em 1em;
      text-decoration: none;
      transition: background 0.25s ease-out;
      text-align: center;
      font-weight: 500;
    }
    .Widgets .button--primary {
      background-color: var(--accent-color, var(--accent, #cdf24b));
    }

    .Widgets svg {
      filter: drop-shadow(0 0 0.5rem rgba(0, 0, 0, 0.25));
    }
    .Widgets .GitHub svg,
    .Widgets .LeetCode svg {
      filter: none;
    }
    .Widgets .theme-fill {
      fill: white;
    }
    .Widgets .theme-stroke {
      stroke: white;
    }

    .background-layer {
      position: absolute;
      inset: 0;
      background-size: cover;
      background-position: center;
      transition: background 0.3s ease, filter 0.3s ease;
    }
    .background-not-configured {
      position: absolute;
      inset: 0;
      display: grid;
      place-items: center;
      color: rgba(255, 255, 255, 0.6);
      font-size: 14px;
      background: #111;
    }

    .container {
      position: relative;
      width: 100%;
      height: 100%;
    }

    .Slot {
      position: absolute;
      pointer-events: none;
    }

    .Slot > * {
      margin: 1rem;
      pointer-events: all;
    }

    .Slot.topLeft {
      top: 0;
      left: 0;
      text-align: left;
    }
    .Slot.topCentre {
      top: 0;
      left: 50%;
      transform: translateX(-50%);
      text-align: center;
    }
    .Slot.topRight {
      top: 0;
      right: 0;
      text-align: right;
    }
    .Slot.middleLeft {
      top: 50%;
      left: 0;
      transform: translateY(-50%);
      text-align: left;
    }
    .Slot.middleCentre {
      top: 50%;
      left: 50%;
      transform: translate(-50%, -50%);
      text-align: center;
    }
    .Slot.middleRight {
      top: 50%;
      right: 0;
      transform: translateY(-50%);
      text-align: right;
    }
    .Slot.bottomLeft {
      bottom: 3rem;
      left: 0;
      text-align: left;
    }
    .Slot.bottomCentre {
      bottom: 3rem;
      left: 50%;
      transform: translateX(-50%);
      text-align: center;
    }
    .Slot.bottomRight {
      bottom: 3rem;
      right: 0;
      text-align: right;
    }

    .Slot.free-slot-wrap {
      position: absolute;
      display: block;
    }
    .Slot.free > * {
      margin: 0;
    }

    .Widget {
      position: relative;
      transition: color 0.15s ease;
      user-select: auto;
    }

    h1, h2, h3, h4 {
      line-height: 1;
      margin: 0;
    }
    .weight-override h1, .weight-override h2, .weight-override h3, .weight-override h4 {
      font-weight: inherit;
    }

    .drag-selected {
      z-index: 1000 !important;
      outline: 2px dashed var(--accent, #cdf24b) !important;
      border-radius: 4px;
      box-shadow: 0 0 0 4px rgba(205, 242, 75, 0.25);
    }
    .drag-selected > * {
      pointer-events: none;
    }
    .free-handles-wrap {
      position: absolute;
      inset: -4px;
      pointer-events: none;
      z-index: 1001;
    }
    .free-handle {
      position: absolute;
      width: 12px;
      height: 12px;
      border-radius: 50%;
      background: #fff;
      border: 2px solid #222;
      pointer-events: auto;
      box-shadow: 0 2px 6px rgba(0, 0, 0, 0.35);
    }
    .free-handle.handle-scale {
      bottom: -6px;
      right: -6px;
      cursor: nwse-resize;
    }
    .free-handle.handle-rotate {
      top: -18px;
      left: 50%;
      transform: translateX(-50%);
      cursor: grab;
    }
    .free-floating-done {
      position: fixed;
      bottom: 24px;
      left: 50%;
      transform: translateX(-50%);
      z-index: 1100;
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 10px 24px;
      background: var(--accent, #cdf24b);
      color: #000;
      font-weight: 700;
      font-size: 14px;
      border: 0;
      border-radius: 999px;
      cursor: pointer;
      box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
      pointer-events: auto;
    }
    .free-floating-done:hover {
      filter: brightness(1.1);
      transform: translateX(-50%) scale(1.03);
    }

    ${backgroundStyles}
    ${widgetStyles}
  `;
}
