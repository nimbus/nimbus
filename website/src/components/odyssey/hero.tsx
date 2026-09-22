'use client';

import { type CSSProperties, type PointerEvent as ReactPointerEvent, type RefObject, useCallback, useEffect, useRef, useState } from 'react';
import { type Viewport, chapterStop, chapters } from './timeline';
import { travelTo } from './journey';
import { Install } from './install';
import { type Part, type PartId, drawFrame, parts, rgb, setFonts } from './world';

import { Mascot } from '@/components/mascot';
import '@/styles/hero.css';

const QUICKSTART = '/get-started/quickstart/';

// An open book, drawn in strokes at the nav's line weight.
function BookIcon() {
  return (
    <svg className="hero-nav-icon" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <path
        d="M1.5 2.75h3.75A2.75 2.75 0 0 1 8 5.5v8.25a2.25 2.25 0 0 0-2.25-2.25H1.5zM14.5 2.75h-3.75A2.75 2.75 0 0 0 8 5.5v8.25a2.25 2.25 0 0 1 2.25-2.25h4.25z"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinejoin="round"
      />
    </svg>
  );
}

// The GitHub mark.
function GitHubIcon() {
  return (
    <svg className="hero-nav-icon" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <path
        fill="currentColor"
        d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0 0 16 8c0-4.42-3.58-8-8-8z"
      />
    </svg>
  );
}
const DOCS = '/docs/';
const GITHUB = 'https://github.com/nimbus/nimbus';

// The splash frames one chapter of the journey below it: the binary, drawn at
// the chapter's own stop, so the diagram the reader lands on is the same
// world they scroll into.
const BINARY = chapters.findIndex((chapter) => chapter.id === 'binary');
const HERO_PROGRESS = chapterStop(BINARY);
// The frame: the app and the binary it calls, centred on the composition and
// zoomed to fit the still. The world span is the drawing's extent plus air.
const HERO_FRAME = { x: 3500, y: 620, w: 7950, h: 3900 };
// A phone frame crops to the binary itself: text keeps its screen-pixel
// floor, so the box needs the width, and the app that calls it is implied.
const HERO_FRAME_COMPACT = { x: 3900, y: 1000, w: 7400, h: 3900 };
const COMPACT_WIDTH = 520;

// The camera for a still of the given size: the frame it shows and the zoom
// that fits it. The scene draws with it, and the pointer maps through it.
function heroCamera(width: number, height: number) {
  const compact = width < COMPACT_WIDTH;
  const frame = compact ? HERO_FRAME_COMPACT : HERO_FRAME;
  const viewport: Viewport = compact ? 'compact' : 'wide';
  return { frame, viewport, zoom: Math.min(width / frame.w, height / frame.h) };
}

// What each part of the still is, for the pointer resting on it. The label
// is the name the drawing gives the part; the description is the claim the
// chapter under it makes.
const PART_COPY: Record<PartId, { label: string; description: string }> = {
  app: {
    label: 'Your app',
    description: 'Existing code with the SDK it already uses. Convex, Firestore, MongoDB, and DynamoDB clients point at one address, and nothing else changes.',
  },
  protocols: {
    label: 'Client protocols',
    description: 'Every SDK has its own endpoint on one host. Each keeps its own wire format and port, and one process answers all of them.',
  },
  adapters: {
    label: 'Backend adapters',
    description: 'Each client protocol is an adapter. It turns wire calls into engine operations and passes an authenticated identity, not a raw token.',
  },
  engine: {
    label: 'Engine',
    description: 'One coordinator for reads, writes, subscriptions, and scheduling. Every protocol meets the same rules and the same database.',
  },
  runtime: {
    label: 'Runtime',
    description: 'Functions run on V8 inside the binary, next to the data. A "use node" directive moves an action to Node with npm packages.',
  },
  node: {
    label: 'Node target',
    description: 'Add "use node" to an action to run it on Node 22, 24, or 26 with npm packages. Native addons and subprocesses need a sandbox.',
  },
  commit: {
    label: 'Commit path',
    description: 'Authorize, validate, then one transaction writes the document, its indexes, and the commit log. Live queries update the moment it commits.',
  },
  database: {
    label: 'Durable state',
    description: 'Documents, KV, and files on one storage layer. SQLite is the default and runs inside the process with zero setup.',
  },
  hosted: {
    label: 'Hosted engines',
    description: 'Point one flag at Postgres, MySQL, or libSQL when you already run one. Every API works on every backend.',
  },
  objects: {
    label: 'Object store',
    description: 'The content-addressed bytes behind files and volumes, encrypted per tenant.',
  },
  files: {
    label: 'Files and volumes',
    description: 'One blob store for S3 objects, the function filesystem, and sandbox volumes. Uploads, the S3 endpoint, and mounts read the same bytes.',
  },
  control: {
    label: 'Control plane',
    description: 'The Nimbus API in the process. It admits SDK calls against the tenant quota and keeps the records for services, sandboxes, and sessions.',
  },
  service: {
    label: 'Service',
    description:
      'A stable entry point for a workload, like a Kubernetes Service. Code and sessions target the name, and Nimbus routes them to the sandbox behind it. Replace the sandbox and the name still resolves; replicas are on the roadmap.',
  },
  sessions: {
    label: 'Sessions',
    description: 'An audited lease on a running service for stdio, file exchange, or browser control. Nimbus authorizes it at open, and it expires at its TTL.',
  },
  host: {
    label: 'Host',
    description: 'The Linux host the binary runs on. Sandboxes and their volumes are carved out of it, outside the Nimbus process.',
  },
  sandbox: {
    label: 'Agent sandbox',
    description: 'Compose gives each agent an OCI image, a microVM or container, and a volume at /work. It has no path back to the engine.',
  },
  isolation: {
    label: 'Isolation',
    description: 'How the host is split, set per sandbox: a container on the shared kernel, or a microVM with its own kernel and memory map.',
  },
  proxy: {
    label: 'Egress proxy',
    description: 'Every request out of a sandbox is denied by default. An allow rule names one protocol, one host, one port, and its paths, with no wildcards.',
  },
  ingress: {
    label: 'Ingress',
    description: 'A listener Nimbus leases on the host for a service, so traffic in reaches the service by name.',
  },
  binary: {
    label: 'One binary',
    description: 'Network, compute, storage, and agents ship in one Rust binary with one trust model. There is no sidecar and no queue to run.',
  },
  network: {
    label: 'Network',
    description: 'Client protocols on one address. Each SDK keeps its own wire format and port, and one process answers all of them.',
  },
  compute: {
    label: 'Compute',
    description: 'V8 and Node targets in the same process. Queries and mutations reach the database with no network hop.',
  },
  storage: {
    label: 'Storage',
    description: 'SQLite by default with zero setup. Postgres, MySQL, or libSQL over a connection. Objects, volumes, and KV on the same layer.',
  },
  agents: {
    label: 'Agents',
    description: 'Sandboxes with egress denied by default. Each agent gets its own microVM or container and its own volume.',
  },
  workloads: {
    label: 'Workloads',
    description: 'Services and sessions. A compose file names each service, and a session is an audited lease on one.',
  },
  auth: {
    label: 'Auth',
    description: 'Principals and tenants, admitted once per request. No API can express a cross-tenant read.',
  },
};

// A part under the pointer, with its box in the still's own pixels, and
// where its card goes: under the box, or over it when the still ends first.
type Hover = { id: PartId; left: number; top: number; width: number; height: number; card: CSSProperties };
const CARD_W = 264;
const CARD_H = 120;
const CARD_GAP = 10;
const EDGE = 8;

function partAt(width: number, height: number, sx: number, sy: number): Hover | null {
  const { frame, viewport, zoom } = heroCamera(width, height);
  const wx = frame.x + (sx - width / 2) / zoom;
  const wy = frame.y + (sy - height / 2) / zoom;
  let hit: Part | undefined;
  for (const part of parts(zoom, viewport)) {
    if (wx >= part.x0 && wx <= part.x1 && wy >= part.y0 && wy <= part.y1) hit = part;
  }
  if (!hit) return null;
  const left = width / 2 + (hit.x0 - frame.x) * zoom;
  const top = height / 2 + (hit.y0 - frame.y) * zoom;
  const w = (hit.x1 - hit.x0) * zoom;
  const h = (hit.y1 - hit.y0) * zoom;
  const cardLeft = Math.min(Math.max(EDGE, left + w / 2 - CARD_W / 2), Math.max(EDGE, width - CARD_W - EDGE));
  const below = top + h + CARD_GAP + CARD_H <= height;
  const card: CSSProperties = below ? { left: cardLeft, top: top + h + CARD_GAP } : { left: cardLeft, bottom: height - top + CARD_GAP };
  return { id: hit.id, left, top, width: w, height: h, card };
}

function resolveFonts() {
  const styles = getComputedStyle(document.body);
  setFonts(styles.getPropertyValue('--font-mono'), styles.getPropertyValue('--font-sans'));
}

// The scene on the splash: the binary chapter, drawn as a centred still and
// kept alive at half rate while it is on screen, so the request rides the
// lanes. Reduced motion gets the still alone.
function mountScene(root: HTMLElement, canvas: HTMLCanvasElement) {
  const ctx = canvas.getContext('2d', { alpha: false });
  if (!ctx) return () => {};
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');

  let width = 0;
  let height = 0;
  let dpr = 1;
  let frameId = 0;
  let running = false;
  let inView = true;
  let lastDraw = 0;

  const draw = (time: number) => {
    if (!width || !height) return;
    const { frame, viewport, zoom } = heroCamera(width, height);
    const { palette } = drawFrame(ctx, {
      width,
      height,
      dpr,
      progress: HERO_PROGRESS,
      time,
      viewport,
      ambient: !reduced.matches,
      centered: true,
      camera: { x: frame.x, y: frame.y, zoom, anchorX: 0.5, anchorY: 0.5 },
    });
    root.style.setProperty('--hero-background', rgb(palette.background));
    root.style.setProperty('--hero-ink', rgb(palette.ink));
    root.style.setProperty('--hero-muted', rgb(palette.muted));
    root.style.setProperty('--hero-accent', rgb(palette.accent));
    root.style.setProperty('--hero-accent-carrier', rgb(palette.accentCarrier));
  };

  const tick = (now: number) => {
    frameId = requestAnimationFrame(tick);
    if (now - lastDraw < 28) return;
    lastDraw = now;
    draw(now);
  };

  const stop = () => {
    running = false;
    cancelAnimationFrame(frameId);
  };

  const start = () => {
    if (running || !inView || document.hidden || reduced.matches) return;
    running = true;
    frameId = requestAnimationFrame(tick);
  };

  const measure = () => {
    width = canvas.clientWidth;
    height = canvas.clientHeight;
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    draw(performance.now());
  };

  const onVisibility = () => {
    if (document.hidden) stop();
    else start();
  };
  const onMotion = () => {
    stop();
    draw(performance.now());
    start();
  };

  const observer = new IntersectionObserver(
    ([entry]) => {
      inView = entry.isIntersecting;
      if (inView) start();
      else stop();
    },
    { threshold: 0 },
  );
  observer.observe(canvas);
  const resizeObserver = new ResizeObserver(measure);
  resizeObserver.observe(canvas);
  document.addEventListener('visibilitychange', onVisibility);
  reduced.addEventListener('change', onMotion);

  measure();
  start();

  return () => {
    stop();
    observer.disconnect();
    resizeObserver.disconnect();
    document.removeEventListener('visibilitychange', onVisibility);
    reduced.removeEventListener('change', onMotion);
  };
}

// The still with the pointer over it: the part under the cursor gets an
// outline and a card that names it. Touch gets the same from a tap.
function Scene({ root }: { root: RefObject<HTMLElement | null> }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [hover, setHover] = useState<Hover | null>(null);
  const current = useRef<PartId | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!root.current || !canvas) return;
    resolveFonts();
    return mountScene(root.current, canvas);
  }, [root]);

  const onPointer = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    const next = partAt(box.width, box.height, event.clientX - box.left, event.clientY - box.top);
    if ((next?.id ?? null) === current.current) return;
    current.current = next?.id ?? null;
    setHover(next);
  }, []);
  // A finger leaves the moment it lifts, so a tap keeps its card until the
  // next tap lands somewhere else.
  const onLeave = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (event.pointerType === 'touch') return;
    current.current = null;
    setHover(null);
  }, []);
  useEffect(() => {
    if (!hover) return;
    const figure = canvasRef.current?.parentElement;
    const onOutside = (event: PointerEvent) => {
      if (figure && event.target instanceof Node && figure.contains(event.target)) return;
      current.current = null;
      setHover(null);
    };
    document.addEventListener('pointerdown', onOutside);
    return () => document.removeEventListener('pointerdown', onOutside);
  }, [hover]);

  const copy = hover ? PART_COPY[hover.id] : null;
  return (
    <figure
      className="hero-scene"
      role="img"
      aria-label="One binary: network, compute, storage, agents, and workloads in one process."
      onPointerMove={onPointer}
      onPointerDown={onPointer}
      onPointerLeave={onLeave}
      onPointerCancel={onLeave}
    >
      <canvas ref={canvasRef} aria-hidden="true" />
      {hover && copy && (
        <>
          <span className="hero-part" aria-hidden="true" style={{ left: hover.left, top: hover.top, width: hover.width, height: hover.height }} />
          <div className="hero-tip" aria-hidden="true" style={hover.card}>
            <strong>{copy.label}</strong>
            <p>{copy.description}</p>
          </div>
        </>
      )}
    </figure>
  );
}

export function Hero() {
  const rootRef = useRef<HTMLElement>(null);
  return (
    <section className="hero" ref={rootRef} aria-labelledby="hero-title">
      <header className="hero-nav">
        <a className="hero-brand" href="/" aria-label="Nimbus home">
          <Mascot className="hero-mark" />
          <span className="wordmark">nimbus</span>
          <small>beta</small>
        </a>
        <nav aria-label="Site">
          <a href={DOCS} target="_blank" rel="noreferrer">
            <BookIcon />
            Docs
          </a>
          <a href={GITHUB} target="_blank" rel="noreferrer">
            <GitHubIcon />
            GitHub
          </a>
          <a className="hero-run" href={QUICKSTART} target="_blank" rel="noreferrer">
            Run locally ↗
          </a>
        </nav>
      </header>

      <div className="hero-body">
        <div className="hero-copy">
          <h1 id="hero-title">
            <span>Own your backend.</span>
            <span>Keep your SDK.</span>
            <span>Sandbox agent sessions.</span>
            <span>Run any workload.</span>
          </h1>
          <p className="hero-lede">
            Nimbus is a single-binary backend for apps and AI agents, drop-in compatible with Convex, Firestore,
            MongoDB, and DynamoDB.
          </p>
          <div className="hero-actions">
            <a className="primary" href={QUICKSTART} target="_blank" rel="noreferrer">
              Get started ↗
            </a>
            <a href={DOCS} target="_blank" rel="noreferrer">Read the docs ↗</a>
          </div>
          <Install />
        </div>

        <Scene root={rootRef} />
      </div>

      <button type="button" className="hero-scroll" onClick={() => travelTo(0)}>
        <span>Scroll to learn more</span>
        <i className="hero-scroll-ring" aria-hidden="true">
          <svg viewBox="0 0 16 16" width="18" height="18">
            <path d="M3.5 6.5 8 11l4.5-4.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </i>
      </button>
    </section>
  );
}
