// The draw context shared by every part of the world: the palette for the
// current progress, the camera-aware canvas helpers, and the backdrop. World
// geometry and the story itself live in world.ts.

import { type Camera, type Viewport, clamp, mix, paperMixFor, washMixFor, range, smoothstep } from './timeline';

export type Rgb = [number, number, number];

export type Palette = {
  background: Rgb;
  // The chrome ground: the background at its stepped tone, so the nav, the
  // run link and the playhead stay in contrast through a crossfade.
  chromeBackground: Rgb;
  ink: Rgb;
  muted: Rgb;
  // The accent carries two jobs the Nimbus palette answers with two tokens:
  // `accent` is the display tone that fills chips, lanes and diamonds, and
  // `accentText` is the reading tone for accent-coloured labels.
  accent: Rgb;
  accentText: Rgb;
  paperMix: number;
  washMix: number;
};

export type FrameOptions = {
  width: number;
  height: number;
  dpr: number;
  progress: number;
  time: number;
  viewport: Viewport;
  ambient: boolean;
  // Storyboard stills centre the camera target instead of leaving room for copy.
  centered?: boolean;
  // A rich still keeps its detail text when the frame forces a lower zoom.
  forceRich?: boolean;
  // The zoom the chapter was composed at, before the stage pulled the camera
  // back to fit; small labels fade by it.
  lodZoom?: number;
};

export type PaletteStops = {
  night: Rgb;
  paper: Rgb;
  // The finale wash: one accent-coloured ground under one ink. The palette
  // allows the gold at this size only as a fill, so the wash carries its own
  // ink instead of mixing the frame's toward white.
  wash: Rgb;
  washInk: Rgb;
  inkNight: Rgb;
  inkPaper: Rgb;
  accentNight: Rgb;
  accentPaper: Rgb;
  accentTextNight: Rgb;
  accentTextPaper: Rgb;
};

// The Nimbus role tokens, read off `src/styles/tokens.css`. The journey has
// no palette of its own: every frame is a mix of these ten, which is why
// retheming the twenty chapters was a change of tones and not of geometry.
//
// Canvas cannot read a CSS variable, so the values are literal here. A change
// to the token sheet belongs in this table too.
const STOPS: PaletteStops = {
  night: [10, 11, 12], // --bg-canvas dark, #0a0b0c
  paper: [250, 250, 250], // --bg-panel light, #fafafa
  // --accent, #f0b23e, under --accent-ink, #1a1204 — 10.9:1.
  wash: [240, 178, 62],
  washInk: [26, 18, 4],
  inkNight: [246, 247, 248], // --text-1 dark, #f6f7f8
  inkPaper: [24, 24, 27], // --text-1 light, #18181b
  // Both accent jobs take the edge token. The world draws no large accent
  // fill: a chip is a 1px stroke over a 12% tint and a panel is a stroke, so
  // every accent in the frame is a thin one and has to be perceivable on its
  // own. The gold clears that floor on the night ground and does not on the
  // paper one, so the paper side darkens at the same hue, exactly as the
  // console does. The display and reading tones therefore coincide here, as
  // they did for the cobalt this palette replaced.
  accentNight: [240, 178, 62], // --accent-edge dark, #f0b23e
  accentPaper: [134, 100, 35], // --accent-edge light, #866423
  accentTextNight: [240, 178, 62], // --accent-edge dark, #f0b23e
  accentTextPaper: [134, 100, 35], // --accent-edge light, #866423
};

// The two stops the world reaches for outside a mixed palette: the laptop
// screen it paints under the frame, and the terminal ink on it.
export function night(): Rgb {
  return STOPS.night;
}

export function inkNight(): Rgb {
  return STOPS.inkNight;
}

// Terminal output on the laptop screen inside the frame. The screen is the
// night stop whatever the surrounding frame is doing, so its brightest ink is
// a literal and not a role token.
export const WHITE: Rgb = [255, 255, 255];

// Canvas fonts cannot read CSS variables; the page resolves the loaded
// families once and hands them over.
let monoFamily = 'ui-monospace, monospace';
let sansFamily = 'system-ui, sans-serif';

export function setFonts(mono: string, sans: string) {
  if (mono.trim()) monoFamily = mono;
  if (sans.trim()) sansFamily = sans;
}

// Per-viewport layout: the few pieces of the world that need a different
// composition on a phone, and the zoom range over which small labels fade.
export type Layout = {
  doorGap: number;
  // Doors widen where the camera sits further back, so the minimum screen font
  // size still fits the port label inside the box.
  doorW: number;
  // Camera zoom range over which small labels fade out (level of detail).
  lod: [number, number];
};

export const layouts: Record<Viewport, Layout> = {
  wide: { doorGap: 150, doorW: 240, lod: [0.55, 0.8] },
  medium: { doorGap: 150, doorW: 280, lod: [0.3, 0.5] },
  compact: { doorGap: 128, doorW: 300, lod: [0.3, 0.42] },
};

export function mixRgb(from: Rgb, to: Rgb, amount: number): Rgb {
  return [mix(from[0], to[0], amount), mix(from[1], to[1], amount), mix(from[2], to[2], amount)];
}

export function rgb(color: Rgb) {
  return `rgb(${color[0].toFixed(0)} ${color[1].toFixed(0)} ${color[2].toFixed(0)})`;
}

export function rgba(color: Rgb, alpha: number) {
  return `rgb(${color[0].toFixed(0)} ${color[1].toFixed(0)} ${color[2].toFixed(0)} / ${clamp(alpha).toFixed(3)})`;
}

export function paletteFor(progress: number): Palette {
  const paperMix = paperMixFor(progress);
  const washMix = washMixFor(progress);
  const background = mixRgb(mixRgb(STOPS.night, STOPS.paper, paperMix), STOPS.wash, washMix);
  // The ink swaps in one step as the background passes its midpoint, so the
  // two never meet at the same grey and the frame keeps its contrast through
  // a crossfade. Any continuous mix passes through that grey; a step does not.
  const inkMix = paperMix < 0.5 ? 0 : 1;
  const ink = mixRgb(mixRgb(STOPS.inkNight, STOPS.inkPaper, inkMix), STOPS.washInk, washMix);
  const chromeBackground = mixRgb(mixRgb(STOPS.night, STOPS.paper, inkMix), STOPS.wash, washMix);
  // On the wash the ground is already the accent, so both accent jobs step
  // aside for its ink. Gold on gold is not a tone.
  const accent = mixRgb(mixRgb(STOPS.accentNight, STOPS.accentPaper, paperMix), STOPS.washInk, washMix);
  const accentText = mixRgb(mixRgb(STOPS.accentTextNight, STOPS.accentTextPaper, paperMix), STOPS.washInk, washMix);
  return {
    background,
    chromeBackground,
    ink,
    muted: mixRgb(chromeBackground, ink, 0.56),
    accent,
    accentText,
    paperMix,
    washMix,
  };
}

export type TrafficOptions = {
  count?: number;
  speed?: number;
  phase?: number;
  size?: number;
  reverse?: boolean;
};

// One coordinate of a cubic bezier at t.
function bezier(t: number, p0: number, p1: number, p2: number, p3: number) {
  const u = 1 - t;
  return u * u * u * p0 + 3 * u * u * t * p1 + 3 * u * t * t * p2 + t * t * t * p3;
}

// Draw context: one per frame. Font sizes are world units but never fall below
// a legible screen size, so the compact camera can zoom out without producing
// unreadable labels.
export class Scene {
  ctx: CanvasRenderingContext2D;
  camera: Camera;
  palette: Palette;
  zoom: number;
  minPx: number;
  viewport: Viewport;
  width: number;
  viewLeft: number;
  viewRight: number;
  viewTop: number;
  viewBottom: number;
  // Level of detail: small labels fade out as the camera pulls back, so the
  // minimum screen font size never makes neighbouring labels collide.
  detail: number;
  // The storage shelf, the sandbox and the service cards are framed from
  // further back than the rest of the story, so they keep their own two
  // levels: titles stay until the camera is far out (`far`), and the detail
  // rows, chips and tags need room a phone never has (`rich`).
  far: number;
  rich: number;
  // Detail text follows `rich` with a steeper curve, so a stage whose zoom
  // rests inside the band shows shrunk panels without half-faded text on
  // them.
  richText: number;
  textAlpha = 1;
  // The frame time in milliseconds, and whether the frame is one of a
  // running sequence. Ambient motion (traffic on the lanes, a blinking
  // caret) follows the time; a static frame draws none of it.
  time: number;
  ambient: boolean;

  constructor(ctx: CanvasRenderingContext2D, options: FrameOptions, camera: Camera, palette: Palette) {
    this.ctx = ctx;
    this.camera = camera;
    this.palette = palette;
    this.zoom = camera.zoom;
    this.time = options.time;
    this.ambient = options.ambient;
    this.minPx = options.viewport === 'compact' ? 10 : 11;
    this.viewport = options.viewport;
    const [lodLow, lodHigh] = layouts[options.viewport].lod;
    this.detail = smoothstep(range(options.lodZoom ?? camera.zoom, lodLow, lodHigh));
    // Titles leave as the camera pulls far out. A phone frames the agent
    // plane at a lower zoom than a desktop, so its band sits lower.
    this.far = options.viewport === 'compact' ? smoothstep(range(camera.zoom, 0.12, 0.18)) : smoothstep(range(camera.zoom, 0.16, 0.22));
    // The band sits between the tablet backends frame (0.5) and the wide
    // services frame (0.53), so every composed frame rests on one side of
    // it. Like detail, it follows the composed zoom: a shorter or narrower
    // stage pulls the camera back to fit, and keeps the card content its
    // chapter was composed with instead of dropping to the bare boxes.
    this.rich = options.forceRich ? 1 : options.viewport === 'compact' && !options.centered ? 0 : smoothstep(range(options.lodZoom ?? camera.zoom, 0.5, 0.525));
    this.richText = smoothstep(range(this.rich, 0.5, 1));
    const { width, height } = options;
    this.width = width;
    this.viewLeft = camera.x - (camera.anchorX * width) / camera.zoom;
    this.viewRight = camera.x + ((1 - camera.anchorX) * width) / camera.zoom;
    this.viewTop = camera.y - (camera.anchorY * height) / camera.zoom;
    this.viewBottom = camera.y + ((1 - camera.anchorY) * height) / camera.zoom;
  }

  inView(x0: number, x1: number, margin = 120, y0 = -600, y1 = 600) {
    return x1 + margin >= this.viewLeft && x0 - margin <= this.viewRight && y1 + margin >= this.viewTop && y0 - margin <= this.viewBottom;
  }

  // Line widths are constant on screen.
  px(value: number) {
    return value / this.zoom;
  }

  fontSize(size: number) {
    return Math.max(size, this.minPx / this.zoom);
  }

  mono(size: number, weight = 500) {
    this.ctx.font = `${weight} ${this.fontSize(size).toFixed(2)}px ${monoFamily}`;
  }

  sans(size: number, weight = 420) {
    this.ctx.font = `${weight} ${this.fontSize(size).toFixed(2)}px ${sansFamily}`;
  }

  screenX(x: number) {
    return this.camera.anchorX * this.width + (x - this.camera.x) * this.zoom;
  }

  text(value: string, x: number, y: number, color: Rgb, alpha: number, align: CanvasTextAlign = 'left') {
    alpha *= this.textAlpha;
    if (alpha <= 0.01) return;
    this.ctx.fillStyle = rgba(color, alpha);
    this.ctx.textAlign = align;
    this.ctx.textBaseline = 'middle';
    this.ctx.fillText(value, x, y);
  }

  roundRect(x: number, y: number, w: number, h: number, radius: number) {
    const r = Math.max(0, Math.min(radius, w / 2, h / 2));
    const { ctx } = this;
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
  }

  panel(cx: number, cy: number, w: number, h: number, alpha: number, emphasis = 0, tint?: Rgb) {
    if (alpha <= 0.01) return;
    const { ctx, palette } = this;
    const x = cx - w / 2;
    const y = cy - h / 2;
    this.roundRect(x, y, w, h, 10);
    ctx.fillStyle = rgba(mixRgb(palette.background, palette.ink, 0.05 + emphasis * 0.05), alpha);
    ctx.fill();
    ctx.lineWidth = this.px(1);
    ctx.strokeStyle = rgba(tint ?? palette.ink, alpha * (0.22 + emphasis * 0.6));
    ctx.stroke();
    if (emphasis > 0.01 && tint) {
      ctx.lineWidth = this.px(2);
      ctx.strokeStyle = rgba(tint, alpha * emphasis * 0.9);
      ctx.stroke();
    }
  }

  // Database symbol: a cylinder with an elliptical lid. It carries the panel
  // fill and line, so an embedded engine, a hosted one and an object store
  // read as one family; where it sits, inside or outside the process line,
  // carries the difference.
  cylinder(cx: number, cy: number, w: number, h: number, alpha: number, emphasis = 0, tint?: Rgb) {
    if (alpha <= 0.01) return;
    const { ctx, palette } = this;
    const rx = w / 2;
    const ry = Math.min(h * 0.12, w * 0.12);
    const top = cy - h / 2 + ry;
    const bottom = cy + h / 2 - ry;
    const line = tint ?? palette.ink;
    ctx.beginPath();
    ctx.moveTo(cx - rx, top);
    ctx.lineTo(cx - rx, bottom);
    ctx.ellipse(cx, bottom, rx, ry, 0, Math.PI, 0, true);
    ctx.lineTo(cx + rx, top);
    ctx.ellipse(cx, top, rx, ry, 0, 0, Math.PI, true);
    ctx.closePath();
    ctx.fillStyle = rgba(mixRgb(palette.background, palette.ink, 0.05 + emphasis * 0.05), alpha);
    ctx.fill();
    ctx.lineWidth = this.px(1);
    ctx.strokeStyle = rgba(line, alpha * (0.22 + emphasis * 0.6));
    ctx.stroke();
    if (emphasis > 0.01 && tint) {
      ctx.lineWidth = this.px(2);
      ctx.strokeStyle = rgba(tint, alpha * emphasis * 0.9);
      ctx.stroke();
    }
    // The lid: a full ellipse, a shade lighter, with its front edge drawn.
    ctx.beginPath();
    ctx.ellipse(cx, top, rx, ry, 0, 0, Math.PI * 2);
    ctx.fillStyle = rgba(mixRgb(palette.background, palette.ink, 0.1 + emphasis * 0.08), alpha);
    ctx.fill();
    ctx.lineWidth = this.px(1);
    ctx.strokeStyle = rgba(line, alpha * (0.22 + emphasis * 0.6));
    ctx.stroke();
  }

  // Top of the body text area inside a cylinder: below the lid's front edge.
  cylinderBodyTop(cy: number, w: number, h: number) {
    const ry = Math.min(h * 0.12, w * 0.12);
    return cy - h / 2 + ry * 3;
  }

  // Bezier lane between two points with a horizontal departure and arrival.
  lane(x0: number, y0: number, x1: number, y1: number, color: Rgb, alpha: number, width = 1, dash?: number[]) {
    if (alpha <= 0.01) return;
    const dx = (x1 - x0) * 0.5;
    this.curve(x0, y0, x0 + dx, y0, x1 - dx, y1, x1, y1, color, alpha, width, dash);
  }

  // Bezier lane with a vertical departure and arrival: down from a box and up
  // into the next one.
  laneVertical(x0: number, y0: number, x1: number, y1: number, color: Rgb, alpha: number, width = 1, dash?: number[]) {
    if (alpha <= 0.01) return;
    const dy = (y1 - y0) * 0.5;
    this.curve(x0, y0, x0, y0 + dy, x1, y1 - dy, x1, y1, color, alpha, width, dash);
  }

  private curve(x0: number, y0: number, c0x: number, c0y: number, c1x: number, c1y: number, x1: number, y1: number, color: Rgb, alpha: number, width: number, dash?: number[]) {
    const { ctx } = this;
    ctx.beginPath();
    ctx.moveTo(x0, y0);
    ctx.bezierCurveTo(c0x, c0y, c1x, c1y, x1, y1);
    ctx.lineWidth = this.px(width);
    ctx.strokeStyle = rgba(color, alpha);
    ctx.setLineDash(dash ? dash.map((value) => this.px(value)) : []);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // Traffic: dots that ride a path while the frame is ambient, as if the
  // system were always busy. Each dot's place is a function of the frame
  // time, so the motion is steady from frame to frame, and a static frame
  // draws none of it. `speed` is cycles per eight seconds; `reverse` sends
  // the dots from the path's end to its start. The dots fade in and out at
  // the ends, so none pops onto or off the lane.
  traffic(path: (t: number) => { x: number; y: number }, color: Rgb, alpha: number, options: TrafficOptions = {}) {
    if (!this.ambient || alpha <= 0.01) return;
    const { count = 2, speed = 2, phase = 0, size = 2.4, reverse = false } = options;
    const { ctx } = this;
    const cycle = this.time * 0.000125 * speed * (reverse ? -1 : 1);
    for (let index = 0; index < count; index += 1) {
      const t = (((phase + index / count + cycle) % 1) + 1) % 1;
      const fade = smoothstep(range(t, 0, 0.12)) * (1 - smoothstep(range(t, 0.88, 1)));
      if (fade <= 0.01) continue;
      const p = path(t);
      ctx.beginPath();
      ctx.arc(p.x, p.y, this.px(size), 0, Math.PI * 2);
      ctx.fillStyle = rgba(color, alpha * fade);
      ctx.fill();
    }
  }

  // Traffic on a horizontal-departure lane, the curve `lane` draws.
  laneTraffic(x0: number, y0: number, x1: number, y1: number, color: Rgb, alpha: number, options: TrafficOptions = {}) {
    const dx = (x1 - x0) * 0.5;
    this.traffic((t) => ({ x: bezier(t, x0, x0 + dx, x1 - dx, x1), y: bezier(t, y0, y0, y1, y1) }), color, alpha, options);
  }

  // Traffic on a vertical-departure lane, the curve `laneVertical` draws.
  laneVerticalTraffic(x0: number, y0: number, x1: number, y1: number, color: Rgb, alpha: number, options: TrafficOptions = {}) {
    const dy = (y1 - y0) * 0.5;
    this.traffic((t) => ({ x: bezier(t, x0, x0, x1, x1), y: bezier(t, y0, y0 + dy, y1 - dy, y1) }), color, alpha, options);
  }

  // Traffic on a straight lane.
  lineTraffic(x0: number, y0: number, x1: number, y1: number, color: Rgb, alpha: number, options: TrafficOptions = {}) {
    this.traffic((t) => ({ x: mix(x0, x1, t), y: mix(y0, y1, t) }), color, alpha, options);
  }

  // A slow breath in [0, 1] for compute that is always running: a cell, an
  // isolate. Zero in a static frame, so the still keeps its composed tone.
  breath(rate = 1, phase = 0) {
    return this.ambient ? 0.5 + 0.5 * Math.sin(this.time * 0.0025 * rate + phase) : 0;
  }

  // A straight lane, dashed or solid, in world units.
  line(x0: number, y0: number, x1: number, y1: number, color: Rgb, alpha: number, width = 1, dash?: number[]) {
    if (alpha <= 0.01) return;
    const { ctx } = this;
    ctx.beginPath();
    ctx.moveTo(x0, y0);
    ctx.lineTo(x1, y1);
    ctx.lineWidth = this.px(width);
    ctx.strokeStyle = rgba(color, alpha);
    ctx.setLineDash(dash ? dash.map((value) => this.px(value)) : []);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  tag(value: string, x: number, y: number, alpha: number, color?: Rgb, align: CanvasTextAlign = 'left') {
    this.mono(11);
    this.text(value, x, y, color ?? this.palette.muted, alpha, align);
  }

  // A small pill-shaped chip with a mono label, sized around the label so the
  // minimum screen font size never spills out of it. The width cap is a screen
  // measure, so a label the chip can hold at one zoom fits at every zoom.
  chip(label: string, x: number, y: number, alpha: number, lit: number, color: Rgb, align: 'left' | 'center' | 'right' = 'left', maxW = this.px(290)) {
    if (alpha <= 0.01) return 0;
    const { ctx, palette } = this;
    this.mono(10.5, 600);
    const w = Math.max(this.px(24), Math.min(ctx.measureText(label).width + this.px(24), maxW));
    const h = this.px(22);
    const left = align === 'center' ? x - w / 2 : align === 'right' ? x - w : x;
    this.roundRect(left, y - h / 2, w, h, h / 2);
    ctx.fillStyle = rgba(mixRgb(palette.background, color, 0.12 + lit * 0.18), alpha);
    ctx.fill();
    ctx.lineWidth = this.px(1);
    ctx.strokeStyle = rgba(color, alpha * (0.3 + lit * 0.6));
    ctx.stroke();
    this.text(label, left + this.px(12), y, mixRgb(palette.muted, palette.ink, 0.4 + lit * 0.6), alpha);
    return w;
  }

  // Width of a label in the current font, for laying tags out beside titles.
  measure(value: string) {
    return this.ctx.measureText(value).width;
  }
}

export function drawBackdrop(ctx: CanvasRenderingContext2D, options: FrameOptions, camera: Camera, palette: Palette) {
  const { width, height, dpr } = options;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = rgb(palette.background);
  ctx.fillRect(0, 0, width, height);

  // Two dot fields at different parallax depths give the travel a floor.
  const layers = [
    { spacing: 56, parallax: 0.22, radius: 0.9, alpha: 0.16 },
    { spacing: 168, parallax: 0.5, radius: 1.6, alpha: 0.2 },
  ];
  ctx.fillStyle = rgba(palette.ink, 1);
  for (const layer of layers) {
    const spacing = layer.spacing;
    const offsetX = (((-camera.x * camera.zoom * layer.parallax) % spacing) + spacing) % spacing;
    const offsetY = (((-camera.y * camera.zoom * layer.parallax) % spacing) + spacing) % spacing;
    ctx.globalAlpha = layer.alpha * (1 - palette.washMix * 0.5);
    ctx.beginPath();
    for (let x = offsetX - spacing; x < width + spacing; x += spacing) {
      for (let y = offsetY - spacing; y < height + spacing; y += spacing) {
        ctx.moveTo(x + layer.radius, y);
        ctx.arc(x, y, layer.radius, 0, Math.PI * 2);
      }
    }
    ctx.fill();
  }
  ctx.globalAlpha = 1;

  // Soft vignette keeps the copy columns calm.
  const vignette = ctx.createRadialGradient(
    width * camera.anchorX,
    height * camera.anchorY,
    Math.min(width, height) * 0.25,
    width * 0.5,
    height * 0.5,
    Math.max(width, height) * 0.85,
  );
  vignette.addColorStop(0, rgba(palette.background, 0));
  vignette.addColorStop(1, rgba(palette.background, 0.7));
  ctx.fillStyle = vignette;
  ctx.fillRect(0, 0, width, height);
}
