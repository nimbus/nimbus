'use client';

import { useEffect, useRef, useState, type CSSProperties } from 'react';
import {
  type Snippet,
  type Viewport,
  actFor,
  actSpans,
  COPY_EDGE,
  chapterIndexFor,
  chapterRest,
  chapterStop,
  chapters,
  clamp,
  interpolateCamera,
  mix,
  range,
  requestStateFor,
  requestStates,
  smoothstep,
  viewportFor,
  visibilityWindow,
} from './timeline';
import { drawFrame, rgb, setFonts } from './world';

import { Mascot } from '@/components/mascot';
import '@/styles/odyssey.css';

function resolveFonts() {
  const styles = getComputedStyle(document.body);
  setFonts(styles.getPropertyValue('--font-mono'), styles.getPropertyValue('--font-sans'));
}

// The journey lives at the site root, so its links are site-relative. Only
// the two that leave the site are absolute.
const QUICKSTART = '/get-started/quickstart/';
const DOCS = '/docs/';
const GITHUB = 'https://github.com/nimbus/nimbus';
const DISCUSS = 'https://github.com/nimbus/nimbus/discussions';

// Reduced motion, or a viewport too short to stage the scene, gets the static
// storyboard: the same world, drawn once per chapter. A phone stacks copy above
// the world, so it needs more height than a desktop stage. Keep in step with
// the same query in `src/styles/odyssey.css`.
const STATIC_QUERY = '(prefers-reduced-motion: reduce), (max-height: 620px), (max-width: 759px) and (max-height: 779px)';
// The storage stills (06, 08) reach below the process edge to the hosted
// database and the bucket, so their frames get more height.
const TALL_STILLS = new Set([6, 8, 9, 10, 11, 12, 17, 18]);
const STILL_PROGRESS = [0.0297, 0.0803, 0.1308, 0.1815, 0.232, 0.2825, 0.3331, 0.3836, 0.4342, 0.4848, 0.5353, 0.5859, 0.6364, 0.687, 0.7376, 0.7881, 0.8387, 0.8892, 0.9398, 0.9881];

// A code snippet beside each chapter: the shortest honest version of the call
// the chapter is about. Tokens are classed for colour, not parsed; the aim is
// a calm block that reads at a glance.
const KEYWORDS = new Set(['import', 'from', 'export', 'const', 'await', 'async', 'return', 'new', 'default']);
const TOKEN = /((?<=^|\s)\/\/.*$|(?<=^|\s)#.*$|"[^"]*"|`[^`]*`|\b[A-Za-z_$][\w$]*\b|\d[\w.]*|…|\s+|[^\s\w"`]+)/g;

function classify(token: string, shell: boolean, first: boolean) {
  if (token === '…') return 'tk-ellipsis';
  if (token.startsWith('//') || token.startsWith('#')) return 'tk-comment';
  if (token.startsWith('"') || token.startsWith('`')) return 'tk-string';
  if (shell && first && token === '$') return 'tk-prompt';
  if (KEYWORDS.has(token)) return 'tk-keyword';
  if (/^[A-Z_]{3,}$/.test(token)) return 'tk-env';
  return '';
}

function CodeSnippet({ snippet }: { snippet: Snippet }) {
  const shell = snippet.file === 'shell';
  return (
    <pre className="snippet" aria-label={shell ? 'Shell commands' : `Code from ${snippet.file}`}>
      <span className="snippet-file" aria-hidden="true">
        {snippet.file}
      </span>
      {[snippet.lines, snippet.compact ?? snippet.lines].map((lines, variant) => (
      <code key={variant} className={variant === 0 ? 'snippet-wide' : 'snippet-compact'} aria-hidden={variant === 1 ? true : undefined}>
        {lines.map((line, lineIndex) => {
          const tokens = line.match(TOKEN) ?? [];
          let first = true;
          return (
            <span className="line" key={lineIndex} style={{ '--i': lineIndex } as React.CSSProperties}>
              {tokens.length === 0 ? '\u00a0' : null}
              {tokens.map((token, tokenIndex) => {
                const className = classify(token, shell, first);
                if (token.trim()) first = false;
                return className ? (
                  <i className={className} key={tokenIndex}>
                    {token}
                  </i>
                ) : (
                  token
                );
              })}
            </span>
          );
        })}
      </code>
      ))}
    </pre>
  );
}

function easeInOutCubic(t: number) {
  return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}

// The scroll driver. On a fine pointer the wheel is taken over. Inside a
// chapter's rest each notch moves a target by this much of its native
// distance, and the page follows the target as a critically damped spring
// with this angular frequency (per second). The spring keeps the velocity
// continuous across notches, so the beats flow under the scroll instead of
// pulsing with each notch, and a small input makes a small, smooth move.
const WHEEL_GAIN = 0.5;
const GLIDE_OMEGA = 11;
// A chapter holds at the edge of its rest. The gesture that brought the reader
// to the edge does not push through it. A later gesture that pushes this far
// (in native wheel pixels, within this window) carries the reader on to the
// next chapter; a gentle nudge does not.
const PUSH_THRESHOLD = 45;
const PUSH_WINDOW = 800;
// And a push counts only once the edge has held this long.
const EDGE_HOLD = 110;
// A new wheel gesture starts after this long with no wheel input, when the
// direction flips, or when the deltas rise again by this ratio: a trackpad's
// momentum tail only decays, so a rise is a new swipe, even inside the tail.
const GESTURE_GAP = 160;
const GESTURE_RISE = 1.5;
// The carry between two chapters: an eased travel that takes about a second,
// and up to half a second more when the camera has far to go. The camera
// moves only in the gap between the chapters, while the copy is off stage,
// so the carry spends most of its time there: the gap weighs this much more
// than the copy's fade at either end. The wheel stays quiet until the gesture
// that started the carry has ended.
const CARRY_DURATION = 1000;
const CARRY_EXTRA = 500;
const GAP_WEIGHT = 8;
// A scroll the driver did not make (scrollbar, keyboard, touch) has ended
// when this long passes with no further movement.
const REST_DEBOUNCE = 140;
// Tooling that sets the scroll position itself puts this attribute on <html>,
// and the driver leaves the scroll alone.
const HOLD_ATTRIBUTE = 'data-journey-hold';

function scrollsWithin(target: EventTarget | null, deltaY: number, stopAt: HTMLElement) {
  let node = target instanceof Element ? target : null;
  while (node && node !== stopAt && node !== document.body) {
    const { overflowY } = getComputedStyle(node);
    if ((overflowY === 'auto' || overflowY === 'scroll') && node.scrollHeight > node.clientHeight) {
      if (deltaY > 0 ? node.scrollTop + node.clientHeight < node.scrollHeight - 1 : node.scrollTop > 0) return true;
    }
    node = node.parentElement;
  }
  return false;
}

type JourneyHandles = {
  section: HTMLElement;
  stage: HTMLElement;
  canvas: HTMLCanvasElement;
  percent: HTMLElement | null;
  track: HTMLElement;
  playhead: HTMLElement;
  frame: HTMLElement;
  onChapter: (index: number) => void;
  onState: (label: string) => void;
};

// The scrolling journey. Scroll position is the playhead; the scene follows it
// with framerate-independent smoothing. The scroll driver below gives the
// scroll its weight and rests it on a chapter. Every DOM write is guarded so
// an idle frame writes nothing.
function mountJourney(handles: JourneyHandles) {
  const { section, stage, canvas, track, playhead, frame } = handles;
  const ctx = canvas.getContext('2d', { alpha: false });
  if (!ctx) return () => {};

  const beats = Array.from(section.querySelectorAll<HTMLElement>('[data-beat]'));
  // The final beat starts as "live" so the first pass makes it inert unless the
  // page loads at the end; its links must not take focus while invisible.
  const beatState = beats.map((_, index) => ({ opacity: -1, shift: 0, live: index === beats.length - 1 }));
  const finalIndex = beats.length - 1;
  const finalBeat = beats[finalIndex];
  const vars = new Map<string, string>();
  const setVar = (name: string, value: string) => {
    if (vars.get(name) === value) return;
    vars.set(name, value);
    stage.style.setProperty(name, value);
  };

  let width = 0;
  let height = 0;
  let dpr = 1;
  let viewport: Viewport = 'wide';
  let sectionTop = 0;
  let distance = 1;
  let frameW = 0;
  let frameH = 0;
  let perimeter = 1;

  let target = 0;
  let progress = 0;
  let lastTime = 0;
  let frameId = 0;
  let running = false;
  let inView = true;
  let lastDraw = 0;
  let driving = false;
  let travelId = 0;
  let gliding = false;
  let glideTo = 0;
  let glideAt = 0;
  let glideVelocity = 0;
  let expectedY = -1;
  let lastY = window.scrollY;
  let direction = 1;
  let touching = false;
  let restTimer = 0;
  let wheelAt = -Infinity;
  let lastDelta = 0;
  let gesture = 0;
  let edgeGesture = -1;
  let edgeAt = -Infinity;
  let push = 0;
  let pushAt = -Infinity;
  let pushDirection = 0;
  let locked = false;
  let scrubbing = false;
  let scrubPointer = -1;
  let scrubShift = 0;
  let chapter = -1;
  let stateLabel = '';
  let percentText = '';

  const readScroll = () => {
    target = clamp((window.scrollY - sectionTop) / distance);
  };

  const applyChrome = (palette: ReturnType<typeof drawFrame>['palette']) => {
    setVar('--stage-background', rgb(palette.background));
    setVar('--stage-chrome', rgb(palette.chromeBackground));
    setVar('--stage-ink', rgb(palette.ink));
    setVar('--stage-muted', rgb(palette.muted));
    setVar('--stage-accent', rgb(palette.accent));
    setVar('--stage-accent-text', rgb(palette.accentText));
    // How far into the finale wash the frame is. The nav mark reads it to
    // trade its body and face over, because the wash ground is the accent.
    setVar('--wash-mix', palette.washMix.toFixed(4));
    setVar('--journey-progress', progress.toFixed(4));
    setVar('--scroll-hint-opacity', (1 - smoothstep(range(progress, 0, 0.03))).toFixed(3));

    // The frame cursor travels the perimeter exactly once over the journey.
    const along = progress * perimeter;
    let cursorX = 0;
    let cursorY = 0;
    if (along < frameW) {
      cursorX = along;
    } else if (along < frameW + frameH) {
      cursorX = frameW;
      cursorY = along - frameW;
    } else if (along < frameW * 2 + frameH) {
      cursorX = frameW - (along - frameW - frameH);
      cursorY = frameH;
    } else {
      cursorY = frameH - (along - frameW * 2 - frameH);
    }
    setVar('--frame-cursor-x', `${cursorX.toFixed(1)}px`);
    setVar('--frame-cursor-y', `${cursorY.toFixed(1)}px`);

    const percent = `${String(Math.round(progress * 100)).padStart(3, '0')}%`;
    if (percent !== percentText) {
      percentText = percent;
      if (handles.percent) handles.percent.textContent = percent;
      playhead.setAttribute('aria-valuenow', String(Math.round(progress * 100)));
    }
    const nextChapter = chapterIndexFor(progress);
    if (nextChapter !== chapter) {
      chapter = nextChapter;
      handles.onChapter(chapter);
    }
    const nextState = requestStateFor(progress);
    if (nextState !== stateLabel) {
      stateLabel = nextState;
      handles.onState(stateLabel);
    }
  };

  const applyBeats = () => {
    beats.forEach((node, index) => {
      const beat = chapters[index];
      // The first beat is already on stage at the top; the last stays on stage
      // at the bottom of the scroll.
      const windowStart = index === 0 ? -1 : beat.start;
      const windowEnd = index === finalIndex ? 2 : beat.end;
      const alpha = visibilityWindow(progress, windowStart, windowEnd, COPY_EDGE);
      const phase = clamp((progress - beat.start) / (beat.end - beat.start));
      const shift = mix(26, -26, phase) * (1 - alpha);
      const state = beatState[index];
      if (Math.abs(alpha - state.opacity) > 0.004 || Math.abs(shift - state.shift) > 0.4) {
        state.opacity = alpha;
        state.shift = shift;
        node.style.opacity = alpha.toFixed(3);
        node.style.transform = `translate3d(0, ${shift.toFixed(1)}px, 0)`;
      }
      const live = alpha > 0.5;
      if (live !== state.live) {
        state.live = live;
        node.classList.toggle('is-live', live);
        if (node === finalBeat) node.inert = !live;
      }
    });
  };

  const draw = (now: number) => {
    const { palette } = drawFrame(ctx, {
      width,
      height,
      dpr,
      progress,
      time: now,
      viewport,
      ambient: true,
    });
    applyChrome(palette);
    applyBeats();
  };

  const step = (dt: number) => {
    if (gliding) {
      const seconds = dt / 1000;
      const gap = glideTo - glideAt;
      glideVelocity += (GLIDE_OMEGA * GLIDE_OMEGA * gap - 2 * GLIDE_OMEGA * glideVelocity) * seconds;
      glideAt += glideVelocity * seconds;
      if (Math.abs(glideTo - glideAt) < 0.5 && Math.abs(glideVelocity) < 20) {
        glideAt = glideTo;
        glideVelocity = 0;
        gliding = false;
      }
      setScroll(glideAt);
    }
    // The scroll is already smooth while the driver moves it; the scene
    // follows it closely. A native scroll gets the fuller smoothing.
    const tau = scrubbing ? 30 : gliding || driving ? 40 : viewport === 'compact' ? 70 : 105;
    progress += (target - progress) * (1 - Math.exp(-dt / tau));
    if (Math.abs(target - progress) < 0.0003) progress = target;
  };

  const stop = () => {
    running = false;
    cancelAnimationFrame(frameId);
  };

  const tick = (now: number) => {
    frameId = requestAnimationFrame(tick);
    const dt = clamp(now - lastTime, 1, 64);
    lastTime = now;
    step(dt);
    // Settled: the scene still runs (traffic rides the lanes), at half
    // rate to spare the battery. The loop rests only when the stage leaves
    // the view or the tab hides.
    const settled = progress === target && !driving && !gliding && !scrubbing;
    if (settled && now - lastDraw < 28) return;
    lastDraw = now;
    draw(now);
  };

  const start = () => {
    if (running || !inView || document.hidden) return;
    running = true;
    lastTime = performance.now();
    frameId = requestAnimationFrame(tick);
  };

  const measure = () => {
    width = stage.clientWidth;
    height = stage.clientHeight;
    viewport = viewportFor(width);
    // Bound the backing store: at most ~4.2M device pixels, never above 2x.
    dpr = Math.min(window.devicePixelRatio || 1, 2, Math.sqrt(4_200_000 / Math.max(1, width * height)));
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    sectionTop = section.getBoundingClientRect().top + window.scrollY;
    distance = Math.max(1, section.offsetHeight - height);
    frameW = frame.clientWidth;
    frameH = frame.clientHeight;
    perimeter = Math.max(1, 2 * (frameW + frameH));
    readScroll();
    if (!running) {
      progress = target;
      draw(performance.now());
    }
  };

  const root = document.documentElement;
  const finePointer = window.matchMedia('(pointer: fine)').matches;
  const held = () => root.hasAttribute(HOLD_ATTRIBUTE);
  const limit = () => Math.max(0, root.scrollHeight - window.innerHeight);
  const scrollFor = (progress: number) => Math.round(sectionTop + progress * distance);

  // A chapter's rest in scroll pixels. The first chapter's rest reaches up to
  // the top of the page; the last chapter's reaches down through the
  // afterword to the end of the page. Whole pixels, so that a position the
  // driver has set lands inside the rest and not a fraction outside it.
  const restBounds = (index: number) => {
    const rest = chapterRest(index);
    return {
      index,
      from: index === 0 ? 0 : scrollFor(rest.from),
      to: index === chapters.length - 1 ? limit() : scrollFor(rest.to),
    };
  };

  // The rest that holds a scroll position, or null in a gap.
  const restAround = (y: number) => {
    for (let index = 0; index < chapters.length; index += 1) {
      const rest = restBounds(index);
      if (y >= rest.from - 1 && y <= rest.to + 1) return rest;
    }
    return null;
  };

  // From a gap, the rest the journey carries the reader to: the next
  // chapter's arrival going forward, the previous chapter's finished scene
  // going back. Null inside a rest.
  const carryFrom = (y: number, heading: number) => {
    for (let index = 0; index < chapters.length; index += 1) {
      const rest = restBounds(index);
      if (y >= rest.from - 1 && y <= rest.to + 1) return null;
      if (y < rest.from) return heading < 0 ? restBounds(index - 1) : rest;
    }
    return null;
  };

  // From a chapter's rest, the rest the journey carries the reader to when
  // they push past its edge. Null at either end of the journey.
  const restBeyond = (index: number, heading: number) => {
    const next = index + (heading < 0 ? -1 : 1);
    if (next < 0 || next >= chapters.length) return null;
    return restBounds(next);
  };

  // Every scroll this code makes is expected, so the scroll listener can tell
  // it from a scroll the reader made with the scrollbar, the keyboard, or a
  // touch.
  const setScroll = (y: number) => {
    expectedY = y;
    window.scrollTo(0, y);
    readScroll();
  };

  // Start or retarget the glide toward a scroll position inside a rest.
  const glide = (to: number) => {
    if (!gliding) {
      glideAt = window.scrollY;
      glideVelocity = 0;
    }
    glideTo = to;
    gliding = true;
    start();
  };

  // An eased scroll that the scene follows, so the browser's smooth scrolling
  // never fights the scene's smoothing. Chapter travel and the carry between
  // chapters both use it.
  const drive = (to: number, duration: number, shape: (t: number) => number = easeInOutCubic) => {
    const from = window.scrollY;
    const delta = to - from;
    interrupt();
    if (Math.abs(delta) < 1) return;
    const startedAt = performance.now();
    driving = true;
    direction = delta > 0 ? 1 : -1;
    const stepDrive = (now: number) => {
      const t = clamp((now - startedAt) / duration);
      setScroll(from + delta * shape(t));
      if (t < 1) {
        travelId = requestAnimationFrame(stepDrive);
      } else {
        driving = false;
      }
    };
    travelId = requestAnimationFrame(stepDrive);
    start();
  };

  // Carry the reader to a rest: the eased travel, with the wheel locked until
  // the gesture that started it has ended, so its momentum does not run on
  // into the chapter that arrives.
  const carryTo = (rest: { index: number; from: number; to: number }) => {
    locked = true;
    push = 0;
    const startY = window.scrollY;
    const heading = rest.from > startY ? 1 : -1;
    const endY = heading > 0 ? rest.from : rest.to;
    const startP = clamp((startY - sectionTop) / distance);
    const endP = clamp((endY - sectionTop) / distance);
    const span = endP - startP;
    if (Math.abs(span) < 1e-6) {
      drive(endY, CARRY_DURATION);
      return;
    }
    // The gap between the two chapters, as fractions of the carry.
    const before = chapters[heading > 0 ? rest.index - 1 : rest.index];
    const after = chapters[heading > 0 ? rest.index : rest.index + 1];
    const gapIn = clamp(((heading > 0 ? before.end : after.start) - startP) / span);
    const gapOut = clamp(((heading > 0 ? after.start : before.end) - startP) / span);
    // Time per unit of travel, and its running sum, so that the carry can be
    // read back from time to position.
    const weight = (u: number) => 1 + (GAP_WEIGHT - 1) * smoothstep(range(u, gapIn - 0.06, gapIn + 0.06)) * (1 - smoothstep(range(u, gapOut - 0.06, gapOut + 0.06)));
    const steps = 96;
    const sums = [0];
    for (let i = 1; i <= steps; i += 1) sums.push(sums[i - 1] + weight((i - 0.5) / steps));
    const total = sums[steps];
    const position = (eased: number) => {
      const wanted = eased * total;
      let i = 1;
      while (i < steps && sums[i] < wanted) i += 1;
      return (i - 1 + (wanted - sums[i - 1]) / (sums[i] - sums[i - 1])) / steps;
    };
    // The camera's travel between the two rests, on screen: a long shift or
    // a big change of zoom earns the carry more time.
    const cameraA = interpolateCamera(startP, viewport);
    const cameraB = interpolateCamera(endP, viewport);
    const shift = ((Math.abs(cameraB.x - cameraA.x) + Math.abs(cameraB.y - cameraA.y)) * Math.min(cameraA.zoom, cameraB.zoom)) / 600;
    const zoomed = Math.abs(Math.log(cameraB.zoom / cameraA.zoom)) / 1.5;
    const duration = CARRY_DURATION + CARRY_EXTRA * clamp(shift + zoomed);
    drive(endY, duration, (t) => position(easeInOutCubic(t)));
  };

  // At rest in a gap between two chapters: carry the reader to the chapter.
  const carry = () => {
    if (held() || driving || gliding) return;
    const rest = carryFrom(window.scrollY, direction);
    if (rest === null) return;
    carryTo(rest);
  };

  const onWheel = (event: WheelEvent) => {
    if (!finePointer || held() || event.ctrlKey || event.metaKey || event.deltaY === 0) return;
    if (scrollsWithin(event.target, event.deltaY, section)) return;
    event.preventDefault();
    // A drag in progress owns the page.
    if (scrubbing) return;
    const now = performance.now();
    const unit = event.deltaMode === 1 ? 32 : event.deltaMode === 2 ? height : 1;
    const delta = event.deltaY * unit;
    const fresh =
      now - wheelAt > GESTURE_GAP ||
      Math.sign(delta) !== Math.sign(lastDelta) ||
      Math.abs(delta) > Math.abs(lastDelta) * GESTURE_RISE + 4;
    wheelAt = now;
    lastDelta = delta;
    if (fresh) gesture += 1;
    // A carry in flight, and the gesture that started it, take the wheel's
    // input without moving. A chapter travel yields to the wheel.
    if (locked) {
      if (driving || !fresh) return;
      locked = false;
    }
    stopTravel();
    direction = delta > 0 ? 1 : -1;
    // A glide in flight carries on from its target; a reversal starts from
    // where the page is now, so it answers at once.
    const from = gliding && Math.sign(glideTo - window.scrollY) === direction ? glideTo : window.scrollY;
    const rest = restAround(from);
    if (rest === null) {
      // In a gap, where only the scrollbar or the keyboard can leave the
      // reader: carry them on now.
      carry();
      return;
    }
    const edge = direction > 0 ? rest.to : rest.from;
    const atEdge = direction > 0 ? from >= edge - 1 : from <= edge + 1;
    if (!atEdge) {
      const to = clamp(from + delta * WHEEL_GAIN, rest.from, rest.to);
      // This gesture reached the edge: it holds there, and does not push on.
      if (to === edge) {
        edgeGesture = gesture;
        edgeAt = now;
      }
      push = 0;
      glide(to);
      return;
    }
    if (gesture === edgeGesture || now - edgeAt < EDGE_HOLD) return;
    if (direction !== pushDirection || now - pushAt > PUSH_WINDOW) push = 0;
    pushDirection = direction;
    pushAt = now;
    push += Math.abs(delta);
    if (push < PUSH_THRESHOLD) return;
    const beyond = restBeyond(rest.index, direction);
    if (beyond === null) return;
    carryTo(beyond);
  };

  const onScroll = () => {
    const y = window.scrollY;
    if (y !== lastY) direction = y > lastY ? 1 : -1;
    lastY = y;
    readScroll();
    start();
    if (Math.abs(y - expectedY) <= 1) {
      expectedY = -1;
      return;
    }
    // The reader moved the page: a glide or a travel in flight yields to
    // them, and when their scroll ends in a gap, the journey carries them to
    // a chapter.
    interrupt();
    window.clearTimeout(restTimer);
    if (!touching) restTimer = window.setTimeout(carry, REST_DEBOUNCE);
  };

  const onTouchStart = () => {
    touching = true;
    interrupt();
  };

  const onTouchEnd = () => {
    touching = false;
    window.clearTimeout(restTimer);
    restTimer = window.setTimeout(carry, REST_DEBOUNCE);
  };

  // Chapter travel from the nav: it lands on the chapter's own stop, and takes
  // longer the further it goes.
  const travel = (index: number) => {
    const to = scrollFor(chapterStop(index));
    const duration = clamp(480 + (Math.abs(to - window.scrollY) / distance) * 1500, 520, 1400);
    locked = false;
    drive(to, duration);
  };

  // Direct manipulation: the reader drags the rail's playhead or the frame's
  // cursor, and the page follows the pointer. The scene follows the page
  // closely, and the drop lands in a chapter the way a scroll does.
  const railProgress = (event: PointerEvent) => {
    const rect = track.getBoundingClientRect();
    return clamp((event.clientX - rect.left) / Math.max(1, rect.width));
  };

  // The point of the frame's perimeter nearest the pointer, as the fraction
  // of the perimeter it lies along. The perimeter is a loop and the journey
  // is not: a drag across the top-left corner stops at the end it came from.
  const frameProgress = (event: PointerEvent) => {
    const rect = frame.getBoundingClientRect();
    const px = event.clientX - rect.left - frame.clientLeft;
    const py = event.clientY - rect.top - frame.clientTop;
    const x = clamp(px, 0, frameW);
    const y = clamp(py, 0, frameH);
    const edges = [
      { gap: Math.abs(py), along: x },
      { gap: Math.abs(px - frameW), along: frameW + y },
      { gap: Math.abs(py - frameH), along: frameW + frameH + (frameW - x) },
      { gap: Math.abs(px), along: frameW * 2 + frameH + (frameH - y) },
    ];
    const nearest = edges.reduce((best, edge) => (edge.gap < best.gap ? edge : best));
    const next = clamp(nearest.along / perimeter);
    if (next - target > 0.5) return 0;
    if (target - next > 0.5) return 1;
    return next;
  };

  const scrubTo = (value: number) => {
    const y = scrollFor(value);
    if (y !== window.scrollY) direction = y > window.scrollY ? 1 : -1;
    setScroll(y);
    start();
  };

  const bindScrub = (handle: HTMLElement, read: (event: PointerEvent) => number) => {
    const onDown = (event: PointerEvent) => {
      if (event.button !== 0 || scrubbing || held()) return;
      event.preventDefault();
      handle.setPointerCapture(event.pointerId);
      interrupt();
      window.clearTimeout(restTimer);
      locked = false;
      push = 0;
      scrubbing = true;
      scrubPointer = event.pointerId;
      // The handle stays under the pointer where it was taken, instead of
      // jumping to the pointer.
      scrubShift = target - read(event);
      stage.classList.add('is-scrubbing');
    };
    const onMove = (event: PointerEvent) => {
      if (!scrubbing || event.pointerId !== scrubPointer) return;
      scrubTo(clamp(read(event) + scrubShift));
    };
    const onUp = (event: PointerEvent) => {
      if (!scrubbing || event.pointerId !== scrubPointer) return;
      scrubbing = false;
      scrubPointer = -1;
      stage.classList.remove('is-scrubbing');
      // Dropped in a gap: the journey carries the reader to a chapter. A touch
      // schedules its own carry when it ends.
      if (!touching) carry();
    };
    handle.addEventListener('pointerdown', onDown);
    handle.addEventListener('pointermove', onMove);
    handle.addEventListener('pointerup', onUp);
    handle.addEventListener('pointercancel', onUp);
    handle.addEventListener('lostpointercapture', onUp);
    return () => {
      handle.removeEventListener('pointerdown', onDown);
      handle.removeEventListener('pointermove', onMove);
      handle.removeEventListener('pointerup', onUp);
      handle.removeEventListener('pointercancel', onUp);
      handle.removeEventListener('lostpointercapture', onUp);
    };
  };

  // The playhead is a slider to the keyboard: it steps by chapter.
  const SLIDER_STEPS = new Map([
    ['ArrowRight', 1],
    ['ArrowUp', 1],
    ['ArrowLeft', -1],
    ['ArrowDown', -1],
  ]);
  const onSliderKey = (event: KeyboardEvent) => {
    const last = chapters.length - 1;
    const stepBy = SLIDER_STEPS.get(event.key);
    let index: number;
    if (event.key === 'Home') index = 0;
    else if (event.key === 'End') index = last;
    else if (stepBy !== undefined) index = clamp(chapter + stepBy, 0, last);
    else return;
    event.preventDefault();
    // The window's keydown ends a travel; this one starts it.
    event.stopPropagation();
    travel(index);
  };

  const stopTravel = () => {
    if (!driving) return;
    cancelAnimationFrame(travelId);
    driving = false;
  };

  // Any other input from the reader ends a glide or a travel in flight.
  const interrupt = () => {
    gliding = false;
    stopTravel();
  };

  const onVisibility = () => {
    if (document.hidden) stop();
    else start();
  };

  const observer = new IntersectionObserver(
    ([entry]) => {
      inView = entry.isIntersecting;
      if (inView) start();
      else stop();
    },
    { threshold: 0 },
  );
  observer.observe(section);

  const resizeObserver = new ResizeObserver(measure);
  resizeObserver.observe(stage);
  window.addEventListener('scroll', onScroll, { passive: true });
  window.addEventListener('wheel', onWheel, { passive: false });
  window.addEventListener('touchstart', onTouchStart, { passive: true });
  window.addEventListener('touchend', onTouchEnd, { passive: true });
  window.addEventListener('touchcancel', onTouchEnd, { passive: true });
  window.addEventListener('keydown', interrupt);
  document.addEventListener('visibilitychange', onVisibility);
  const unbindPlayhead = bindScrub(playhead, railProgress);
  const frameCursor = frame.querySelector<HTMLElement>('i');
  const unbindFrame = frameCursor ? bindScrub(frameCursor, frameProgress) : () => {};
  playhead.addEventListener('keydown', onSliderKey);

  measure();
  start();
  travelRegistry.current = travel;

  return () => {
    stop();
    cancelAnimationFrame(travelId);
    window.clearTimeout(restTimer);
    observer.disconnect();
    resizeObserver.disconnect();
    window.removeEventListener('scroll', onScroll);
    window.removeEventListener('wheel', onWheel);
    window.removeEventListener('touchstart', onTouchStart);
    window.removeEventListener('touchend', onTouchEnd);
    window.removeEventListener('touchcancel', onTouchEnd);
    window.removeEventListener('keydown', interrupt);
    document.removeEventListener('visibilitychange', onVisibility);
    unbindPlayhead();
    unbindFrame();
    playhead.removeEventListener('keydown', onSliderKey);
    stage.classList.remove('is-scrubbing');
    beats.forEach((node) => {
      node.style.opacity = '';
      node.style.transform = '';
      node.classList.remove('is-live');
      node.inert = false;
    });
    travelRegistry.current = travelStatic;
  };
}

// Static storyboard: seven stills from the same world, one per chapter, with
// the tonal arc carried into each card's colours.
function mountStoryboard(section: HTMLElement) {
  const stills = Array.from(section.querySelectorAll<HTMLCanvasElement>('canvas.still'));
  const draw = () => {
    stills.forEach((canvas, index) => {
      const width = canvas.clientWidth;
      const height = canvas.clientHeight;
      const ctx = canvas.getContext('2d', { alpha: false });
      if (!width || !height || !ctx) return;
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      const { palette } = drawFrame(ctx, {
        width,
        height,
        dpr,
        progress: STILL_PROGRESS[index] ?? 0.5,
        time: 0,
        viewport: 'compact',
        ambient: false,
        centered: true,
      });
      const beat = canvas.closest<HTMLElement>('[data-beat]');
      beat?.style.setProperty('--beat-background', rgb(palette.background));
      beat?.style.setProperty('--beat-ink', rgb(palette.ink));
      beat?.style.setProperty('--beat-accent', rgb(palette.accentText));
    });
  };
  draw();
  const resizeObserver = new ResizeObserver(draw);
  resizeObserver.observe(section);
  travelRegistry.current = travelStatic;
  return () => resizeObserver.disconnect();
}

function travelStatic(index: number) {
  const node = document.getElementById(`chapter-${chapters[index].id}`);
  if (!node) return;
  const top = node.getBoundingClientRect().top + window.scrollY - 76;
  window.scrollTo({ top, behavior: 'auto' });
}

const travelRegistry: { current: (index: number) => void } = { current: travelStatic };

export function Journey() {
  const journeyRef = useRef<HTMLElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const percentRef = useRef<HTMLElement>(null);
  const trackRef = useRef<HTMLElement>(null);
  const playheadRef = useRef<HTMLSpanElement>(null);
  const frameRef = useRef<HTMLDivElement>(null);
  const [activeChapter, setActiveChapter] = useState(0);
  const [requestState, setRequestState] = useState(requestStates[0].label);

  useEffect(() => {
    const section = journeyRef.current;
    const stage = stageRef.current;
    const canvas = canvasRef.current;
    const track = trackRef.current;
    const playhead = playheadRef.current;
    const frame = frameRef.current;
    if (!section || !stage || !canvas || !track || !playhead || !frame) return;
    const media = window.matchMedia(STATIC_QUERY);
    let cleanup: (() => void) | undefined;
    const boot = () => {
      cleanup?.();
      resolveFonts();
      cleanup = media.matches
        ? mountStoryboard(section)
        : mountJourney({
            section,
            stage,
            canvas,
            percent: percentRef.current,
            track,
            playhead,
            frame,
            onChapter: setActiveChapter,
            onState: setRequestState,
          });
    };
    boot();
    media.addEventListener('change', boot);
    return () => {
      media.removeEventListener('change', boot);
      cleanup?.();
    };
  }, []);

  const lastIndex = chapters.length - 1;
  const activeAct = actFor(chapters[activeChapter]);
  const spans = actSpans();

  return (
    <>
      <a className="skip-link" href="#afterword">
        Skip to the end
      </a>
      <main className="odyssey" ref={journeyRef} aria-label="Request path through Nimbus">
        <div className="odyssey-stage" ref={stageRef}>
          <canvas className="world" ref={canvasRef} aria-hidden="true" />
          <div className="halftone-field" aria-hidden="true" />

          <header className="nav-shell">
            <button
              type="button"
              className="brand"
              onClick={() => travelRegistry.current(0)}
              aria-label="Nimbus. Return to the start."
            >
              <Mascot className="brand-mark" />
              nimbus
              <small>beta</small>
            </button>
            <nav className="chapter-nav" aria-label="Chapters">
              <button type="button" className="start-button" onClick={() => travelRegistry.current(0)} aria-current={activeChapter === 0 ? 'step' : undefined}>
                <i aria-hidden="true" />
                Start
              </button>
              {spans.map(({ act, start, end }) => {
                const members = chapters.map((beat, index) => ({ beat, index })).filter(({ beat }) => beat.act === act.id);
                const current = activeAct?.id === act.id;
                return (
                  <div key={act.id} className="act-group" data-current={current ? 'true' : undefined} style={{ '--act-start': start, '--act-end': end } as CSSProperties}>
                    <button type="button" className="act-button" onClick={() => travelRegistry.current(members[0].index)} aria-current={current ? 'step' : undefined}>
                      {act.label}
                    </button>
                    <div className="act-chapters">
                      <div className="act-chapters-row">
                        {members.map(({ beat, index }) => (
                          <button key={beat.id} type="button" onClick={() => travelRegistry.current(index)} aria-current={index === activeChapter ? 'step' : undefined}>
                            {beat.label}
                          </button>
                        ))}
                      </div>
                    </div>
                    <span className="act-fill" aria-hidden="true" />
                  </div>
                );
              })}
            </nav>
            <button type="button" className="start-return" onClick={() => travelRegistry.current(0)} aria-label="Return to the start">
              <i aria-hidden="true" />
              Start
            </button>
            <a className="run-link" href={QUICKSTART}>
              Run it locally ↗
            </a>
          </header>

          <div className="beats">
            {chapters.map((beat, index) => (
              <article
                key={beat.id}
                id={`chapter-${beat.id}`}
                data-beat
                className={`beat beat-${beat.align}${index === lastIndex ? ' beat-final' : ''}`}
                style={{ opacity: index === 0 ? 1 : 0, ...(TALL_STILLS.has(index) ? { '--still-aspect': '16 / 12' } : {}) } as CSSProperties}
              >
                <p className="beat-eyebrow">{beat.eyebrow}</p>
                {index === 0 ? <h1>{beat.title}</h1> : <h2>{beat.title}</h2>}
                <p>{beat.copy}</p>
                <CodeSnippet snippet={beat.snippet} />
                <span className="proof">{beat.proof}</span>
                {index === lastIndex ? (
                  <div className="final-actions">
                    <a className="primary" href={QUICKSTART}>
                      Start the quickstart ↗
                    </a>
                    <a href={GITHUB}>Read the source ↗</a>
                  </div>
                ) : null}
                <canvas className="still" aria-hidden="true" />
              </article>
            ))}
          </div>

          <div className="journey-status">
            <div className="status-location">
              <span aria-live="polite" aria-atomic="true" style={{ display: 'contents' }}>
                <b>
                  {String(activeChapter).padStart(2, '0')} / {String(chapters.length - 1).padStart(2, '0')}
                </b>
                <span>{chapters[activeChapter].label}</span>
              </span>
              <span className="state">{requestState}</span>
            </div>
            <nav className="progress-track" aria-label="Progress" ref={trackRef}>
              <span className="played" aria-hidden="true" />
              {spans.map(({ act, start, end }) => (
                <span
                  key={act.id}
                  className="rail-act"
                  aria-hidden="true"
                  style={{ left: `${(start * 100).toFixed(2)}%`, width: `${((end - start) * 100).toFixed(2)}%` }}
                >
                  {act.label}
                </span>
              ))}
              {chapters.map((beat, index) => (
                <button
                  key={beat.id}
                  type="button"
                  style={{ left: `${(beat.start * 100).toFixed(2)}%` }}
                  onClick={() => travelRegistry.current(index)}
                  aria-current={index === activeChapter ? 'step' : undefined}
                  aria-label={index === 0 ? `Start: ${beat.label}` : `Chapter ${index}: ${beat.label}`}
                />
              ))}
              <span
                className="playhead"
                ref={playheadRef}
                role="slider"
                tabIndex={0}
                aria-label="Position"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={0}
                aria-valuetext={chapters[activeChapter].label}
              />
            </nav>
            <div className="telemetry">
              <span>request</span>
              <b ref={percentRef}>000%</b>
            </div>
          </div>

          <div className="scroll-prompt" aria-hidden="true">
            <i />
            <span>Scroll to follow the request</span>
          </div>
          <div className="stage-frame" ref={frameRef} aria-hidden="true">
            <i />
          </div>
        </div>
      </main>

      <footer className="afterword" id="afterword">
        <div className="afterword-row">
          <nav aria-label="Nimbus resources">
            <a href={DOCS}>Docs ↗</a>
            <a href={GITHUB}>GitHub ↗</a>
            <a href={DISCUSS}>Discuss ↗</a>
          </nav>
          <span>nimbus · one binary backend</span>
        </div>
        <div className="afterword-row">
          <p className="fine">
            Nimbus is in beta and has not launched. APIs can break between releases. Do not use it in production
            yet. The source is available under the Nimbus Community License. No telemetry.
          </p>
        </div>
      </footer>
    </>
  );
}
