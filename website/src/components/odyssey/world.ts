// The world owns one continuous scene: a request that leaves an app through
// its Convex client, enters a cloud protocol, becomes an engine operation,
// forks into the runtime where its handler runs, comes back through the engine
// to commit as one storage transaction, lands on the storage backend beneath
// it, goes live, sits beside the byte plane that files and volumes are built
// on, reaches an agent inside a sandbox, is leased through a service and a
// session, is seen to be one process, is walled off in its tenant, lands on
// the developer's cloud, and finally drops down to the laptop where the
// quickstart runs. Everything is drawn from `progress` (0..1) so the scrolling
// stage and the static storyboard share one source of truth.

import { type Viewport, clamp, interpolateCamera, mix, monotoneCubic, range, requestStateFor, smoothstep, visibilityWindow } from './timeline';
import { type FrameOptions, type Layout, type Rgb, Scene, WHITE, drawBackdrop, inkNight, layouts, mixRgb, night, paletteFor, rgba } from './scene';

export { type FrameOptions, type Palette, type Rgb, paletteFor, rgb, setFonts } from './scene';

// Wayfinding, not theming: five protocol doors need five hues a reader can
// tell apart at a glance. The role tokens carry no categorical scale, so this
// one stays as drawn.
const doorColors: Rgb[] = [
  [238, 138, 75],
  [240, 181, 60],
  [145, 166, 255],
  [84, 185, 137],
  [158, 131, 255],
];

// The cloud protocols Nimbus answers on today, with the port each one uses in
// `nimbus dev`. The request in this story is a Convex mutation.
const doors = [
  { name: 'CONVEX', port: ':3210 · /convex', client: 'convex/browser' },
  { name: 'FIRESTORE', port: ':3210 · REST / WebSocket', client: 'firebase/firestore' },
  { name: 'CLOUD FUNCTIONS', port: ':3210 · HTTP', client: 'firebase-functions' },
  { name: 'MONGODB', port: ':27017 · wire protocol', client: 'mongodb' },
  { name: 'DYNAMODB', port: ':8000 · JSON 1.0', client: '@aws-sdk/client-dynamodb' },
];
const REQUEST_DOOR = 0;

const terminalLines = [
  '$ brew install nimbus/tap/nimbus',
  '$ nimbus init convex my-app',
  '$ cd my-app',
  '$ nimbus dev',
  '  Local:      http://localhost:3210',
];

// World geometry. Coordinates are shared by every viewport; the per-viewport
// layout moves the few pieces that need a different composition on a phone.
const APP = { x: 0, y: 0, w: 330, h: 250 };
const DOOR_X = 980;
const DOOR_H = 84;
const BAR = { x: 1480, w: 130 };
const MERGE_X = 1720;
const ENGINE = { x: 2080, y: 0, w: 400, h: 240 };
// The fork: a function call takes the runtime lane; a driver write takes the
// direct lane over it. Both rejoin before authorize.
const FORK_X = ENGINE.x + ENGINE.w / 2;
// The card's top edge stays at -150, so the isolate lane and the pill above
// the card keep their places.
const RUNTIME = { x: 2700, y: -34, w: 360, h: 232 };
const DIRECT_Y = -250;
// The isolate card inside the runtime box, where the request rests while its
// handler runs.
const ISOLATE_Y = -53;
const REJOIN_X = 3160;
const AUTHORIZE = { x: 3400, y: 0, w: 176, h: 70 };
const VALIDATE = { x: 3620, y: 0, w: 176, h: 70 };
const TXN = { x: 3850, w: 220, h: 70, gap: 110 };
const PUBLISH = { x: 4220, y: 0, w: 150, h: 60 };
const FANOUT_X = 4480;
const BINARY = { x0: 800, x1: 7200, y0: -520, y1: 1250 };
// The cloud sits above the process: the binary rises into a rack there. In
// the cluster chapter the rack splits into three, spread across a region
// that widens with them, and the mesh between them is drawn beneath. The
// laptop stays below, where the cable from the region lands.
const CLOUD = { x: 8300, y: -1300, w: 760, h: 700, spreadW: 540, spreadH: 300 };
const RACK = { x: 8300, y: -1280, w: 340, h: 540, slots: 6, spread: 400 };
const RACK_SLOT = 2;
const LAPTOP = { x: 8300, y: 1500, w: 540, h: 300 };
// The operator key card stands right of the laptop lid.
const OPERATOR = { x: 8800, top: 1340, w: 330 };

// The Node target beneath the runtime: what "use node" gives an action module
// and what it withholds. Same isolate, same process; chapter 04 frames it.
const NODE = { x: 2700, top: 250, w: 480 };

// The storage shelf beneath the transaction: the durable engines the one
// storage layer can commit through, and the two planes on the row below.
// Durable state, read top down like an architecture diagram: the planes that
// sit on the storage layer, the layer itself, then the engines beneath it.
// SQLite and redb are libraries in the process and write under the data
// directory. Postgres, MySQL and libSQL are servers the operator runs, so
// they sit below the process edge, one connection away.
const SHELF = { y: 760, h: 56, left: 3750, right: 4806 };
const PLANES = { y: 660, h: 60 };
const planeTiles = [
  { title: 'DOCUMENTS', chip: 'tables · indexes', sub: 'one namespace per tenant', x0: 3750, x1: 4150, emphasis: 0.4 },
  { title: 'KV', chip: 'opt-in · early', sub: 'RESP :6380 · nimbus kv', x0: 4164, x1: 4470, emphasis: 0.2 },
  { title: 'FILES', chip: 'own plane →', sub: 'objects · volumes →', x0: 4484, x1: 4806, emphasis: 0.6 },
];
const ENGINE_TILE = { w: 236, h: 140, embeddedY: 910, hostedY: 1420 };
const embeddedEngines = [
  { name: 'SQLITE', chip: 'default', sub: 'a file in ./data', x: 3870 },
  { name: 'REDB', chip: 'embedded', sub: 'files in ./data', x: 4120 },
];
const hostedEngines = [
  { name: 'POSTGRES', chip: 'schema/tenant', sub: 'a server you operate', x: 4300 },
  { name: 'MYSQL', chip: 'db/tenant', sub: 'a server you operate', x: 4550 },
  { name: 'LIBSQL', chip: 'remote db', sub: 'cache in ./data', x: 4800 },
];
// The commit rests on the default engine's lid.
const SHELF_REST = { x: 3850, y: ENGINE_TILE.embeddedY - ENGINE_TILE.h / 2 + Math.min(ENGINE_TILE.h * 0.12, ENGINE_TILE.w * 0.12) };
const OBJECT_STORE = { x: 5350, y: 1420, w: 330, h: 140 };

// The files panel: one byte plane, with objects, the isolate filesystem and
// named volumes built up on it. Rows are laid out in screen pixels from the
// top, so the panel grows to fit the labels at any zoom.
const FILES = { x: 5350, top: 500, w: 800, inner: 720 };
const FILES_CARD = { w: 340, xs: [5160, 5540] };
const FILES_REST = { x: 4900, y: 560 };

// The agent plane. The box is a container or a microVM that nimbus carves out
// of the host for the compose service `agent`: a separate process, so it sits
// below the process edge on the host ground, beside the bucket. The tenant
// door that admits the SDK call, the proxy, the service that names the box
// and the sessions that lease it stay inside the process above it.
const HOST = { x: 6450, y: 1600, w: 1300, h: 620 };
const SANDBOX = { x: 6180, y: 1550, w: 680, h: 400 };
const AGENT = { x: SANDBOX.x - SANDBOX.w / 2 + 22 + 150, y: SANDBOX.y - 94, w: 300, h: 64 };
const VOLUME = { x: AGENT.x, y: SANDBOX.y - 16, w: 300, h: 64 };
const ROOTFS = { x: SANDBOX.x, w: SANDBOX.w - 44, h: 64 };
const SANDBOX_COLUMN_X = AGENT.x + AGENT.w / 2 + 28;
const SANDBOX_REST = { x: AGENT.x + AGENT.w / 2 - 24, y: AGENT.y };
// The image pipeline on the host ground under the box, and the two kinds of
// wall beside it.
const PIPELINE = { y: 1830, w: 270, h: 64, xs: [5955, 6250, 6545] };
const WALL = { x: 6870, w: 340, top: SANDBOX.y - SANDBOX.h / 2 };
// The tenant door: the control plane, the Nimbus API in the process, admits
// the SDK call against the tenant's quota, keeps the service, sandbox and
// session records, and creates the box below. The request waits there while
// the box is framed, then rises to the service. The card stands between the
// volume lane and the lane the service backs the box with.
const CONTROL = { x: 5980, top: 920, w: 380 };
const DOOR_REST = { x: CONTROL.x, y: CONTROL.top - 30 };
// The lane that carries the named volume from the files plane into the box.
const VOLUME_LANE_X = 5770;
// The egress path: a gate on the box wall, an orthogonal lane up across the
// process edge into the proxy, and the one host a rule names at the right edge.
const EGRESS = { corner: 6560, x1: 7080 };
const PROXY = { x: 6760, y: 1000, w: 520 };
// Ingress: the way in from outside the process to a box-backed service. A
// door on the right edge, opposite the protocol doors: a host TCP listener
// nimbus owns, leased for the service, which is the published endpoint. The
// door sits level with the service card at every detail level, so the lane
// runs from the door into the card, and from the card's underside drops
// onto the guest port in the box top, between the lane the service backs
// and the by-id lease. A box on its own publishes nothing.
const INGRESS = { x: 7020, y: 506, w: 240, landX: 6300 };
// The service and the sessions are control-plane resources, so they sit
// inside the process, in its lower right corner directly above the box they
// govern, between the tenant door and the proxy. The service card holds
// screen-sized rows: at detail level it keeps a screen-sized minimum width
// and grows leftward from a fixed right edge, which stays clear of the lease
// by id. The lane it backs the box with drops from a fixed column, right of
// the tenant door.
const SERVICE = { right: 6420, top: 480, w: 520, lane: 6180 };
// The request rests beside the service, right of the lease that drops past
// it, then turns under the card onto the lane the service backs.
const SERVICE_REST = { x: 6660, y: SERVICE.top + 160 };
const SERVICE_TURN_Y = SERVICE.top + 320;
// How far below the process edge the hosted engines, the object store and the
// host ground reach: the module row and the tenant labels hang beneath them.
const EXTERNAL_DEPTH = Math.max(ENGINE_TILE.hostedY + ENGINE_TILE.h / 2, HOST.y + HOST.h / 2) + 40 - BINARY.y1;
// Sessions sit above the service: an architecture stack, top to bottom. Two
// lease the service by name and drop onto it; the third leases the box by id
// and drops past the service's right edge, between it and the proxy.
const SESSIONS = { top: 240, w: 232, xs: [5960, 6210, 6460] };
// The workloads board right of the host, and where the request docks on it:
// right of the compose card, level with its title row.
const WORKLOADS = { x: 7760, y: 1610, w: 1000, h: 720 };
const WORKLOADS_REST = { x: 8080, y: 1332 };

// Tenants at the scale of the process: everything past the fork is one
// tenant's slice of the same machinery, drawn on the front sheet of a deck.
// Two more sheets sit behind it, offset up and to the left, inside the
// process outline and clear of the engine.
const SHEET = { x0: 2500, x1: 7170, y0: -410, y1: 1190 };
const SHEETS = [
  { id: 'demo', note: 'this request', owns: '' },
  { id: 'acme', note: '', owns: 'own database · own blob store · own runtime budget' },
  { id: '_nimbus', note: 'reserved', owns: 'system tenant · operator only' },
];

type RoutePoint = { x: number; y: number; at: number };

function rackSlotY(slot: number) {
  return RACK.y - RACK.h / 2 + (slot + 0.5) * (RACK.h / RACK.slots);
}

function appChipY(index: number) {
  return APP.y - 56 + index * 38;
}

function routeFor(layout: Layout): RoutePoint[] {
  const slotY = rackSlotY(RACK_SLOT);
  const born = appChipY(REQUEST_DOOR);
  const g = layout.doorGap;
  return [
    { x: APP.x - 10, y: born, at: 0.0061 },
    { x: APP.x + APP.w / 2, y: born, at: 0.0399 },
    { x: 620, y: -1.8 * g, at: 0.0578 },
    { x: DOOR_X - layout.doorW / 2, y: -2 * g, at: 0.0714 },
    { x: DOOR_X + layout.doorW / 2, y: -2 * g, at: 0.0882 },
    { x: BAR.x - BAR.w / 2, y: -g, at: 0.1088 },
    { x: BAR.x + BAR.w / 2, y: -0.6 * g, at: 0.1202 },
    { x: MERGE_X, y: 0, at: 0.1277 },
    { x: ENGINE.x, y: 0, at: 0.1424 },
    // The fork: this is a function call, so it takes the runtime lane and runs
    // its handler. The handler holds the isolate through the functions and
    // the Node chapters: the Node target is the same isolate, not a trip.
    { x: FORK_X, y: 0, at: 0.1517 },
    { x: RUNTIME.x - RUNTIME.w / 2, y: 0, at: 0.1612 },
    { x: RUNTIME.x, y: ISOLATE_Y, at: 0.1687 },
    { x: RUNTIME.x, y: ISOLATE_Y, at: 0.2422 },
    { x: RUNTIME.x + RUNTIME.w / 2 + 20, y: -20, at: 0.2502 },
    // Back through the engine: the mutation the action runs rejoins the
    // direct lane and commits.
    { x: REJOIN_X, y: 0, at: 0.2528 },
    { x: AUTHORIZE.x, y: 0, at: 0.2577 },
    { x: VALIDATE.x, y: 0, at: 0.2699 },
    { x: TXN.x, y: 0, at: 0.2829 },
    { x: TXN.x, y: 0, at: 0.2927 },
    // Publish: the sealed commit fans out. The request follows the
    // subscribers lane and docks at the edge of that card; the other two
    // targets light up beneath it.
    { x: PUBLISH.x, y: 0, at: 0.3071 },
    { x: FANOUT_X - 150, y: -190, at: 0.3265 },
    { x: FANOUT_X - 150, y: -190, at: 0.3433 },
    // Back around the sealed transaction and straight down onto the default
    // backend beneath it, where the commit rests while the backends copy is
    // up.
    { x: TXN.x + TXN.w / 2 + 60, y: -190, at: 0.3471 },
    { x: TXN.x + TXN.w / 2 + 60, y: TXN.h / 2 + 56, at: 0.3505 },
    { x: TXN.x, y: TXN.h / 2 + 56, at: 0.3523 },
    { x: SHELF_REST.x, y: SHELF_REST.y, at: 0.3608 },
    { x: SHELF_REST.x, y: SHELF_REST.y, at: 0.3853 },
    // Beside the byte plane while the files copy is up.
    { x: FILES_REST.x, y: FILES_REST.y, at: 0.4106 },
    { x: FILES_REST.x, y: FILES_REST.y, at: 0.4444 },
    // The action's second call, nimbus.sessions.open, arrives at the tenant
    // door and waits there while the box below is framed and its egress is
    // shown. Then it rises to the service that resolves the name, turns under
    // the card onto the lane the service backs, rides it down into the box,
    // and holds there while the camera pulls back to frame the whole binary
    // and the tenant deck.
    { x: DOOR_REST.x, y: DOOR_REST.y, at: 0.4657 },
    { x: DOOR_REST.x, y: DOOR_REST.y, at: 0.5592 },
    { x: SERVICE_REST.x, y: SERVICE_REST.y, at: 0.571 },
    { x: SERVICE_REST.x, y: SERVICE_REST.y, at: 0.5985 },
    // The workloads board: the request docks beside the compose file while
    // the services it names light up, then returns to the service for the
    // sessions chapter.
    { x: WORKLOADS_REST.x, y: WORKLOADS_REST.y, at: 0.6099 },
    { x: WORKLOADS_REST.x, y: WORKLOADS_REST.y, at: 0.6451 },
    { x: SERVICE_REST.x, y: SERVICE_REST.y, at: 0.6555 },
    { x: SERVICE_REST.x, y: SERVICE_REST.y, at: 0.6675 },
    { x: SERVICE_REST.x, y: SERVICE_TURN_Y, at: 0.6704 },
    { x: SERVICE.lane, y: SERVICE_TURN_Y, at: 0.6735 },
    { x: SERVICE.lane, y: SANDBOX.y - SANDBOX.h / 2 + 30, at: 0.6815 },
    { x: SANDBOX_REST.x, y: SANDBOX_REST.y, at: 0.6862 },
    { x: SANDBOX_REST.x, y: SANDBOX_REST.y, at: 0.7983 },
    // Then rise into the rack in the cloud. This host keeps its place when
    // the cluster spreads, so the request stays in its slot.
    { x: 7650, y: -900, at: 0.8136 },
    { x: RACK.x - RACK.w / 2 + 40, y: slotY, at: 0.828 },
    { x: RACK.x - RACK.w / 2 + 40, y: slotY, at: 0.8995 },
    // The descent: out of the slot, down to the region floor, then straight
    // down the cable to the laptop screen. The operator chapter holds it on
    // the lid.
    { x: RACK.x - RACK.w / 2 + 40, y: RACK.y + RACK.h / 2 + 30, at: 0.9013 },
    { x: LAPTOP.x, y: CLOUD.y + CLOUD.h / 2 + CLOUD.spreadH + 40, at: 0.9033 },
    { x: LAPTOP.x, y: LAPTOP.y - LAPTOP.h / 2 - 40, at: 0.9101 },
    { x: LAPTOP.x + LAPTOP.w / 2 - 36, y: LAPTOP.y - LAPTOP.h / 2 + 32, at: 0.916 },
  ];
}

type Route = { times: number[]; xs: number[]; ys: number[] };

function routeTrack(points: RoutePoint[]): Route {
  return { times: points.map((p) => p.at), xs: points.map((p) => p.x), ys: points.map((p) => p.y) };
}

function pointOnRoute(route: Route, progress: number) {
  return { x: monotoneCubic(route.times, route.xs, progress), y: monotoneCubic(route.times, route.ys, progress) };
}

// Every object the binary frames comes back while the camera pulls out to
// show one process, and stays through the tenants chapter until the outline
// collapses into the rack, even if its own chapter has ended and it left the
// stage to keep clear of the next copy block.
function revisit(progress: number) {
  return visibilityWindow(progress, 0.6972, 0.795, 0.019);
}

// The live copy lands over the engine's row on a desktop and a tablet, so
// what sits on that row left of the sealed commit leaves with the commit
// chapter and returns with the revisit.
function liveClear(progress: number) {
  return Math.max(1 - smoothstep(range(progress, 0.2927, 0.3033)), revisit(progress));
}

function stage(progress: number, start: number, end: number, edge = 0.04) {
  return Math.max(visibilityWindow(progress, start, end, edge), revisit(progress));
}

// The app the developer already has: one card, five SDKs it may be using. The
// code itself lives in the copy column beside it; the world shows the shape.
function drawApp(scene: Scene, progress: number, layout: Layout) {
  const alpha = stage(progress, -0.095, 0.099);
  if (alpha <= 0.01 || !scene.inView(APP.x - 220, APP.x + 400)) return;
  const { palette } = scene;
  const x = APP.x - APP.w / 2;
  const y = APP.y - APP.h / 2;
  scene.textAlpha = scene.detail;

  scene.panel(APP.x, APP.y, APP.w, APP.h, alpha, 0.2, palette.accent);
  scene.mono(12, 620);
  scene.text('APP', x + 20, y + 24, palette.ink, alpha);
  scene.mono(10.5);
  scene.text('existing code · no changes', x + 20, y + 44, palette.muted, alpha * 0.85);
  scene.line(x, y + 62, x + APP.w, y + 62, palette.ink, alpha * 0.14);

  // SDK chips light up one after another on arrival; the one the request
  // leaves through stays lit.
  const arrive = smoothstep(range(progress, 0, 0.0229));
  const born = smoothstep(range(progress, 0.003, 0.0167));
  doors.forEach((door, index) => {
    const cy = appChipY(index);
    const reveal = clamp(arrive * 2.2 - index * 0.3);
    const lit = index === REQUEST_DOOR ? born : 0;
    scene.chip(door.client, x + 20, cy, alpha * reveal, lit, index === REQUEST_DOOR ? palette.accent : doorColors[index]);
  });

  // Lanes from the app to the doors. Every SDK has its own door; the request's
  // lane is the bright one.
  const laneAlpha = alpha * Math.max(visibilityWindow(progress, 0.0266, 0.099, 0.0285), revisit(progress));
  if (laneAlpha > 0.01) {
    doors.forEach((door, index) => {
      const dy = (index - 2) * layout.doorGap;
      const isRequest = index === REQUEST_DOOR;
      scene.lane(x + APP.w, appChipY(index), DOOR_X - layout.doorW / 2, dy, isRequest ? palette.accent : doorColors[index], laneAlpha * (isRequest ? 0.6 : 0.28), isRequest ? 1.4 : 1);
      // Every SDK keeps talking to its door.
      scene.laneTraffic(x + APP.w, appChipY(index), DOOR_X - layout.doorW / 2, dy, isRequest ? palette.accent : doorColors[index], laneAlpha * (isRequest ? 0.9 : 0.55), { speed: 1.6 + index * 0.25, phase: index * 0.37 });
    });
  }
  scene.textAlpha = 1;
}

function drawDoors(scene: Scene, progress: number, layout: Layout) {
  // The merged lane outlives the doors: the request rides it into the engine.
  const mergeAlpha = visibilityWindow(progress, 0.0969, 0.7983, 0.0285);
  if (mergeAlpha > 0.01 && scene.inView(MERGE_X, ENGINE.x)) {
    scene.lane(MERGE_X + 70, 0, ENGINE.x - ENGINE.w / 2, 0, scene.palette.accent, mergeAlpha * 0.8, 1.6);
    scene.laneTraffic(MERGE_X + 70, 0, ENGINE.x - ENGINE.w / 2, 0, scene.palette.accent, mergeAlpha * 0.9, { count: 3, speed: 3 });
  }

  // The doors leave before the edge chapter's copy lands over them; the bar
  // and the merged lane stay, since the edge chapter frames them.
  const alpha = stage(progress, 0.0428, 0.114);
  const barAlpha = stage(progress, 0.0844, 0.7983, 0.0285);
  if (Math.max(alpha, barAlpha) <= 0.01 || !scene.inView(APP.x, BAR.x + 200)) return;
  const { ctx, palette } = scene;
  scene.textAlpha = scene.detail;
  // Port labels sit at the minimum screen size when the camera is far back,
  // so the door widens around its centre to keep them inside.
  scene.mono(11);
  const doorW = mix(layout.doorW, Math.max(layout.doorW, Math.max(...doors.map((door) => scene.measure(door.port))) + 60), scene.detail);
  const doorLeft = DOOR_X - doorW / 2;
  const doorRight = DOOR_X + doorW / 2;

  // The heading over the doors: your protocols, one address. The phone
  // composition has no room above the doors and the headline already says it.
  const headAlpha = scene.viewport === 'compact' ? 0 : alpha * visibilityWindow(progress, 0.0484, 0.0947, 0.0285);
  scene.mono(11, 620);
  scene.text('CLIENT PROTOCOLS · ONE ADDRESS', doorLeft, -2 * layout.doorGap - DOOR_H / 2 - scene.px(22), palette.ink, headAlpha);

  doors.forEach((door, index) => {
    const dy = (index - 2) * layout.doorGap;
    const color = doorColors[index];
    const isRequestDoor = index === REQUEST_DOOR;
    if (alpha <= 0.01 && !isRequestDoor) return;
    const passing = isRequestDoor ? visibilityWindow(progress, 0.0665, 0.0947, 0.0285) : 0;
    // Doors wake up one by one as the camera arrives.
    const wake = clamp(smoothstep(range(progress, 0.0446, 0.0727)) * 2 - index * 0.25);

    scene.panel(DOOR_X, dy, doorW, DOOR_H, alpha * wake, 0.25 + passing * 0.75, color);
    ctx.fillStyle = rgba(color, alpha * wake * (0.6 + passing * 0.4));
    ctx.fillRect(doorLeft + 14, dy - 20, 4, 40);
    scene.mono(12, 560);
    scene.text(door.name, doorLeft + 30, dy - 12, palette.ink, alpha * wake);
    scene.tag(door.port, doorLeft + 30, dy + 13, alpha * wake * 0.9);

    // Lane from the door into the adapter bar; lanes converge behind the bar.
    // The lanes leave with the doors, so none runs under the edge copy.
    const laneAlpha = alpha * barAlpha;
    scene.lane(doorRight, dy, BAR.x - BAR.w / 2, dy * 0.55, isRequestDoor ? palette.accent : color, laneAlpha * (isRequestDoor ? 0.7 : 0.35), isRequestDoor ? 1.4 : 1);
    scene.laneTraffic(doorRight, dy, BAR.x - BAR.w / 2, dy * 0.55, isRequestDoor ? palette.accent : color, laneAlpha * (isRequestDoor ? 0.9 : 0.6), { speed: 2 + index * 0.2, phase: index * 0.29 });
  });

  // Adapter bar: where each client protocol is translated. It appears as
  // the camera leaves the doors for the engine.
  if (barAlpha > 0.01) {
    const barH = layout.doorGap * 2.2 + 120;
    scene.panel(BAR.x, 0, BAR.w, barH, barAlpha, 0.15);
    scene.mono(11, 600);
    ctx.save();
    ctx.translate(BAR.x, 0);
    ctx.rotate(-Math.PI / 2);
    scene.text('CLIENT ADAPTERS', 0, 0, palette.ink, barAlpha * 0.9, 'center');
    ctx.restore();
    doors.forEach((_, index) => {
      const dy = (index - 2) * layout.doorGap * 0.55;
      const isRequest = index === REQUEST_DOOR;
      scene.lane(BAR.x + BAR.w / 2, dy * 0.6, MERGE_X, 0, isRequest ? palette.accent : palette.muted, barAlpha * (isRequest ? 0.8 : 0.35), isRequest ? 1.4 : 1);
      scene.laneTraffic(BAR.x + BAR.w / 2, dy * 0.6, MERGE_X, 0, isRequest ? palette.accent : palette.muted, barAlpha * (isRequest ? 0.9 : 0.6), { count: 1, speed: 2.4 + index * 0.2, phase: index * 0.23 });
    });
    scene.lane(MERGE_X, 0, MERGE_X + 70, 0, palette.accent, barAlpha * 0.8, 1.6);
    // The tag sits under the lane between the bar and the engine, and stays
    // out where the phone's text floor makes it wider than that gap.
    scene.mono(11);
    const edgeGap = ENGINE.x - ENGINE.w / 2 - scene.px(10) - (BAR.x + BAR.w / 2) - scene.px(8);
    if (scene.measure('token in · principal out') <= edgeGap) {
      scene.tag('token in · principal out', ENGINE.x - ENGINE.w / 2 - scene.px(10), scene.px(26), barAlpha * visibilityWindow(progress, 0.114, 0.147, 0.019), palette.accentText, 'right');
    }
  }
  scene.textAlpha = 1;
}

// The engine, and the fork on its far side: a function call goes to the
// runtime first; a driver write goes straight on to the commit.
function drawEngine(scene: Scene, progress: number) {
  const alpha = visibilityWindow(progress, 0.0905, 0.7983, 0.038);
  if (alpha <= 0.01 || !scene.inView(ENGINE.x - 260, REJOIN_X + 200)) return;
  const { ctx, palette } = scene;
  const inside = visibilityWindow(progress, 0.1202, 0.1548, 0.0285);
  // Admitted once: the tenants cell lights again while the tenant deck is up.
  const admit = visibilityWindow(progress, 0.7618, 0.7983, 0.0114);
  scene.textAlpha = scene.detail;

  const x = ENGINE.x - ENGINE.w / 2;
  const y = ENGINE.y - ENGINE.h / 2;
  // The concepts the engine owns flow into rows by measured width: two a row
  // on a desktop, and wrapped where a phone's text floor makes them wider.
  // The box grows downward to hold the wrapped rows.
  const cells = ['reads', 'writes', 'subscriptions', 'schedules', 'principals', 'tenants'];
  const innerW = ENGINE.w - 40;
  const twoUp = (innerW - 4) / 2;
  // Screen-pixel sizes are capped, so the box stays a box when the camera
  // frames the whole binary and the text has left.
  const cellH = Math.max(34, Math.min(scene.px(16), 40));
  scene.mono(11.5);
  const rowEnd = x + 20 + innerW + 0.5;
  const placed: { cell: string; x: number; y: number; w: number; row: number }[] = [];
  let cellX = x + 20;
  let rowY = y + 76;
  let rowIndex = 0;
  cells.forEach((cell) => {
    const natural = scene.far > 0.5 ? scene.measure(cell) + scene.px(20) : 0;
    // Half the row where that fits beside its neighbour, else its own width.
    const cellW = natural <= twoUp && cellX + twoUp <= rowEnd ? twoUp : Math.min(natural, innerW);
    if (cellX + cellW > rowEnd) {
      cellX = x + 20;
      rowY += cellH + 12;
      rowIndex += 1;
    }
    placed.push({ cell, x: cellX, y: rowY, w: cellW, row: rowIndex });
    cellX += cellW + 4;
  });
  const boxH = Math.max(ENGINE.h, rowY + cellH / 2 + 20 - y);
  scene.panel(ENGINE.x, y + boxH / 2, ENGINE.w, boxH, alpha, 0.3 + inside * 0.5 + admit * 0.3, palette.accent);
  scene.mono(12, 620);
  scene.text('ENGINE', x + 20, y + 22, palette.ink, alpha);
  scene.mono(10.5);
  const subFull = 'nimbus-engine · one coordinator';
  const sub = scene.measure(subFull) <= ENGINE.w - 40 ? subFull : 'one coordinator';
  scene.text(sub, x + 20, y + 44, palette.muted, alpha * 0.8);

  scene.mono(11.5);
  placed.forEach(({ cell, x: cellX, y: rowY, w: cellW, row }, index) => {
    const door = cell === 'tenants' ? admit : 0;
    const lit = Math.max(clamp(inside * 1.6 - row * 0.3), door);
    // The engine is never idle: each cell breathes on its own beat.
    const hum = scene.breath(0.5, index * 1.7) * 0.05;
    scene.roundRect(cellX, rowY - cellH / 2, cellW, cellH, 6);
    ctx.fillStyle = rgba(palette.accent, alpha * (lit * 0.14 + door * 0.3 + hum));
    ctx.fill();
    ctx.lineWidth = scene.px(1);
    ctx.strokeStyle = rgba(mixRgb(palette.ink, palette.accent, door), alpha * (0.14 + door * 0.6));
    ctx.stroke();
    scene.text(cell, cellX + 12, rowY + 1, mixRgb(palette.muted, palette.ink, lit), alpha);
  });

  // The runtime lane: a function call runs its handler first.
  const runtimeLane = alpha * visibilityWindow(progress, 0.138, 0.7983, 0.0285);
  scene.lane(FORK_X, 0, RUNTIME.x - RUNTIME.w / 2, 0, palette.accent, runtimeLane * 0.7, 1.6);
  scene.laneTraffic(FORK_X, 0, RUNTIME.x - RUNTIME.w / 2, 0, palette.accent, runtimeLane * 0.9, { speed: 3, phase: 0.5 });

  // The direct lane: a driver write has no handler, so it goes over the
  // runtime and rejoins the same path before authorize.
  // The descent to the rejoin waits for the node copy to leave, since it
  // would run under the functions and node copy on a desktop.
  const direct = alpha * visibilityWindow(progress, 0.1451, 0.7983, 0.0285) * 0.5 * liveClear(progress);
  const descent = alpha * visibilityWindow(progress, 0.2485, 0.7983, 0.019) * 0.5 * liveClear(progress);
  const dash = [6, 8];
  scene.line(FORK_X, -70, RUNTIME.x - RUNTIME.w / 2 - 20, DIRECT_Y, palette.muted, direct, 1.2, dash);
  scene.line(RUNTIME.x - RUNTIME.w / 2 - 20, DIRECT_Y, RUNTIME.x + RUNTIME.w / 2 + 80, DIRECT_Y, palette.muted, direct, 1.2, dash);
  // Driver writes keep coming over the top.
  scene.lineTraffic(RUNTIME.x - RUNTIME.w / 2 - 20, DIRECT_Y, RUNTIME.x + RUNTIME.w / 2 + 80, DIRECT_Y, palette.muted, direct * 1.6, { count: 1, speed: 1.4, phase: 0.2 });
  scene.line(RUNTIME.x + RUNTIME.w / 2 + 80, DIRECT_Y, REJOIN_X, 0, palette.muted, descent, 1.2, dash);
  // On a phone the lane runs behind the copy column, so the tag stays out.
  const tagAlpha = scene.viewport === 'compact' ? 0 : direct * 1.6;
  scene.tag('driver write · skips the runtime', (RUNTIME.x - RUNTIME.w / 2 - 20 + RUNTIME.x + RUNTIME.w / 2 + 80) / 2, DIRECT_Y - scene.px(14), tagAlpha, palette.muted, 'center');
  scene.textAlpha = 1;
}

// Compute: the V8 runtime beside the engine, with the durable schedule
// beneath, the rejoin back into the engine's path, and the lane down to the
// host for work that is not TypeScript.
function drawRuntime(scene: Scene, progress: number) {
  const alpha = stage(progress, 0.1354, 0.7983) * liveClear(progress);
  if (alpha <= 0.01 || !scene.inView(NODE.x - NODE.w / 2 - 200, REJOIN_X + 200, 120, -600, NODE.top + 400)) return;
  const { ctx, palette } = scene;
  scene.textAlpha = scene.detail;
  const x = RUNTIME.x - RUNTIME.w / 2;
  const y = RUNTIME.y - RUNTIME.h / 2;
  const inside = visibilityWindow(progress, 0.1548, 0.198, 0.0285);

  scene.panel(RUNTIME.x, RUNTIME.y, RUNTIME.w, RUNTIME.h, alpha, 0.3 + inside * 0.5, palette.accent);
  scene.mono(12, 620);
  scene.text('RUNTIME', x + 20, y + 22, palette.ink, alpha);
  scene.mono(10.5);
  scene.text('V8 via deno_core · same process', x + 20, y + 44, palette.muted, alpha * 0.8);

  // The isolate: one function, lit while the request runs through it.
  const run = visibilityWindow(progress, 0.1614, 0.192, 0.019);
  // Handlers keep running between requests: the isolate breathes.
  const busy = scene.breath(0.7, 1.3) * 0.05;
  scene.roundRect(x + 20, y + 66, RUNTIME.w - 40, 62, 8);
  ctx.fillStyle = rgba(palette.accent, alpha * (run * 0.14 + busy));
  ctx.fill();
  ctx.lineWidth = scene.px(1);
  ctx.strokeStyle = rgba(palette.ink, alpha * 0.16);
  ctx.stroke();
  scene.mono(11.5, 600);
  scene.text('isolate', x + 34, y + 84, mixRgb(palette.muted, palette.ink, run), alpha);
  scene.mono(11);
  scene.text('messages.send(ctx, args)', x + 34, y + 108, mixRgb(palette.muted, palette.ink, run), alpha);

  // The compatibility target: the web-standard isolate by default, or a Node
  // target when the module says "use node". Same engine, same process.
  // The chips take their own row under the tag: beside it, the three of them
  // run past the card edge at a desktop zoom.
  const targetY = y + 146;
  scene.tag('"use node" · target', x + 20, targetY, alpha * scene.richText);
  let chipX = x + 20;
  const chipY = targetY + scene.px(24);
  ['node 22', 'node 24', 'node 26'].forEach((label, index) => {
    chipX += scene.chip(label, chipX, chipY, alpha * scene.richText, index === 1 ? 1 : 0.2, palette.accent) + scene.px(6);
  });

  // The host op: ctx.runMutation goes back through the engine, on the same
  // path a driver write takes. The two lanes meet at one node.
  // The rejoin sits under the live copy on a desktop, so it leaves with the
  // commit chapter and returns with the revisit.
  const rejoin = alpha * visibilityWindow(progress, 0.2485, 0.7983, 0.019) * liveClear(progress);
  scene.lane(RUNTIME.x + RUNTIME.w / 2, 0, REJOIN_X, 0, palette.accent, rejoin * 0.7, 1.6);
  scene.lane(REJOIN_X, 0, AUTHORIZE.x - AUTHORIZE.w / 2, 0, palette.accent, rejoin * 0.7, 1.6);
  scene.lineTraffic(RUNTIME.x + RUNTIME.w / 2, 0, AUTHORIZE.x - AUTHORIZE.w / 2, 0, palette.accent, rejoin * 0.9, { count: 3, speed: 3, phase: 0.1 });
  ctx.beginPath();
  ctx.arc(REJOIN_X, 0, scene.px(4), 0, Math.PI * 2);
  ctx.fillStyle = rgba(palette.background, rejoin);
  ctx.fill();
  ctx.lineWidth = scene.px(1.4);
  ctx.strokeStyle = rgba(palette.accent, rejoin);
  ctx.stroke();
  scene.tag('same path', REJOIN_X, scene.px(24), rejoin * scene.richText, palette.accentText, 'center');

  // The Node target: what "use node" adds to an action module, and what it
  // withholds. The card carries chapter 04, so its text reads at that
  // chapter's zoom; the lane ties it to the target row inside the runtime.
  scene.textAlpha = 1;
  const node = alpha * smoothstep(range(progress, 0.1916, 0.205)) * (1 - smoothstep(range(progress, 0.7698, 0.7983)));
  const unfold = smoothstep(range(progress, 0.198, 0.21));
  const nodeRows = [24 + scene.px(22), scene.px(18), scene.px(18), scene.px(24), scene.px(20)];
  const nodeFullH = nodeRows.reduce((sum, row) => sum + row, 0) + scene.px(16);
  const nodeH = mix(52, nodeFullH, scene.rich);
  const nodeLines = ['"use node" · first line of the module', 'npm packages · express · openai · prisma', 'fetch · actions only · under egress policy'];
  const withheldLabel = 'not here · native addons · subprocess · shell';
  // The card grows to the right where the text floor makes its rows wider
  // than its composed width (a narrow tablet), so no row leaves the card.
  scene.mono(10.5);
  const rowW = Math.max(...nodeLines.map((line) => scene.measure(line)));
  scene.mono(10.5, 600);
  const chipW = scene.measure(withheldLabel) + scene.px(24);
  const nodeCardW = Math.max(NODE.w, Math.max(rowW, chipW) * scene.rich + 40);
  const nx = NODE.x - NODE.w / 2;
  const ny = NODE.top;
  scene.line(NODE.x, y + RUNTIME.h, NODE.x, mix(y + RUNTIME.h, ny, unfold), palette.accent, node * unfold * 0.7, 1.6);
  scene.panel(nx + nodeCardW / 2, ny + nodeH / 2, nodeCardW, nodeH, node * unfold, 0.3, palette.accent);
  scene.mono(12, 620);
  scene.text('NODE TARGET', nx + 20, ny + 24, palette.ink, node * unfold * scene.far);
  const nodeW = scene.measure('NODE TARGET');
  scene.chip('node 24 · default', nx + 20 + nodeW + scene.px(12), ny + 24, node * unfold * scene.richText, 1, palette.accent);
  scene.mono(10.5);
  let nodeY = ny + nodeRows[0];
  nodeLines.forEach((line, index) => {
    const show = smoothstep(range(progress, 0.21 + index * 0.003, 0.216 + index * 0.003));
    scene.text(line, nx + 20, nodeY, palette.ink, node * show * scene.richText * 0.85);
    nodeY += nodeRows[index + 1];
  });
  const withheld = smoothstep(range(progress, 0.219, 0.225));
  scene.chip(withheldLabel, nx + 20, nodeY, node * withheld * scene.richText, 0, palette.muted, 'left', nodeCardW - 40);
  nodeY += nodeRows[4];
  scene.tag('a sandbox runs that work · chapter 09', nx + 20, nodeY, node * withheld * scene.richText, palette.accentText);
}

function drawCommit(scene: Scene, progress: number) {
  const alpha = visibilityWindow(progress, 0.2427, 0.7983, 0.038);
  if (alpha <= 0.01 || !scene.inView(AUTHORIZE.x - 200, FANOUT_X + 260)) return;
  const { ctx, palette } = scene;
  scene.textAlpha = scene.detail;
  // The transaction group stays through the live chapter, where the sealed
  // box is the source the publish lane leaves from, and leaves before the
  // backends copy lands over it. The two checks and their lanes leave with
  // the commit copy, so nothing sits under the live copy beside the box. On
  // a phone the backends chapter frames the transaction row behind the copy,
  // so the row leaves once the commit has dropped to the shelf.
  const phoneFade = scene.viewport === 'compact' ? smoothstep(range(progress, 0.3539, 0.3641)) * (1 - smoothstep(range(progress, 0.5919, 0.7052))) : 0;
  const txnGroup = alpha * stage(progress, 0.2427, 0.3683, 0.0114) * (1 - phoneFade);
  const checks = txnGroup * liveClear(progress);

  const steps = [
    { box: AUTHORIZE, title: 'AUTHORIZE', sub: 'principal · tenant', at: 0.2581 },
    { box: VALIDATE, title: 'VALIDATE', sub: 'schema · optional', at: 0.2726 },
  ];
  steps.forEach((step) => {
    const lit = visibilityWindow(progress, step.at - 0.0104, step.at + 0.0342, 0.019);
    scene.panel(step.box.x, step.box.y, step.box.w, step.box.h, checks, 0.2 + lit * 0.6, palette.accent);
    scene.mono(11.5, 620);
    scene.text(step.title, step.box.x, step.box.y - 11, palette.ink, checks, 'center');
    // The card holds its subtitle where it fits; where the text floor makes
    // the subtitle wider than the card (a phone, a narrow tablet) it stays out.
    scene.mono(10.5);
    if (scene.measure(step.sub) <= step.box.w - 16) {
      scene.text(step.sub, step.box.x, step.box.y + 12, palette.muted, checks, 'center');
    }
  });
  scene.lane(AUTHORIZE.x + AUTHORIZE.w / 2, 0, VALIDATE.x - VALIDATE.w / 2, 0, palette.accent, checks * 0.6, 1.6);
  scene.lane(VALIDATE.x + VALIDATE.w / 2, 0, TXN.x - TXN.w / 2 - 14, 0, palette.accent, checks * 0.6, 1.6);
  // Writes keep passing the checks into the transaction.
  scene.lineTraffic(AUTHORIZE.x + AUTHORIZE.w / 2, 0, VALIDATE.x - VALIDATE.w / 2, 0, palette.accent, checks * 0.9, { count: 1, speed: 3.2, phase: 0.4 });
  scene.lineTraffic(VALIDATE.x + VALIDATE.w / 2, 0, TXN.x - TXN.w / 2 - 14, 0, palette.accent, checks * 0.9, { count: 1, speed: 3.2, phase: 0.9 });

  // One storage transaction: the box is there from the chapter's start,
  // named, with its three effects spread inside it. They lock together as
  // the request arrives and the box seals around them.
  const merge = smoothstep(range(progress, 0.2726, 0.2829));
  const parts = ['DOCUMENT WRITE', 'INDEX EFFECTS', 'COMMIT LOG'];
  const partAlpha = 1 - smoothstep(range(merge, 0.2, 0.7));
  parts.forEach((part, index) => {
    const spread = (index - 1) * TXN.gap * (1 - merge);
    scene.panel(TXN.x, spread, TXN.w, TXN.h, txnGroup * (1 - merge * 0.5), 0.2, palette.accent);
    scene.mono(11, 600);
    scene.text(part, TXN.x, spread, palette.ink, txnGroup * partAlpha, 'center');
  });
  const h = TXN.h + TXN.gap * 2 * (1 - merge) + 40 * merge;
  const boxLeft = TXN.x - TXN.w / 2 - 14;
  const boxRight = TXN.x + TXN.w / 2 + 14;
  const boxTop = -h / 2 - 14;
  scene.roundRect(boxLeft, boxTop, TXN.w + 28, h + 28, 14);
  ctx.lineWidth = scene.px(2);
  ctx.strokeStyle = rgba(palette.accent, txnGroup * (0.55 + merge * 0.45));
  ctx.setLineDash([scene.px(6), scene.px(6 * (1 - merge) + 0.01)]);
  ctx.stroke();
  ctx.setLineDash([]);
  scene.mono(11, 620);
  // On a tablet the box sits close beside the copy column in the live
  // chapter, and on a phone just past the frame's left edge, so the name and
  // the contents line, which reach left of the box, leave with the commit
  // copy there. A desktop has the room, and keeps them beside the live copy.
  const namesStay = scene.viewport === 'wide' ? 1 : 1 - smoothstep(range(progress, 0.2927, 0.3033));
  scene.text('DOCUMENT · INDEX · COMMIT LOG', TXN.x, scene.px(24), palette.accentText, txnGroup * smoothstep(range(progress, 0.2789, 0.2842)) * namesStay, 'center');
  // The name sits on the box's top edge, right-aligned so it reads whole
  // beside the live copy and clear of the request pill; the promise sits
  // under the box's bottom-left corner once the box has sealed.
  scene.text('ONE DATABASE TRANSACTION', boxRight, boxTop - scene.px(16), palette.accentText, txnGroup * namesStay, 'right');
  scene.tag('all or nothing', boxLeft, boxTop + h + 28 + scene.px(16), txnGroup * merge);

  // Publish: the sealed commit fans out to everything that listens. The lane
  // leaves the box's right edge, so the box reads as the source.
  const publishAlpha = alpha * stage(progress, 0.2932, 0.3608, 0.0114);
  if (publishAlpha > 0.01) {
    const source = smoothstep(range(progress, 0.2954, 0.3071));
    scene.lane(boxRight, 0, mix(boxRight, PUBLISH.x - PUBLISH.w / 2, source), 0, palette.accent, publishAlpha * source * 0.6, 1.6);
    // Every commit publishes: the lane out of the box never rests.
    scene.lineTraffic(boxRight, 0, mix(boxRight, PUBLISH.x - PUBLISH.w / 2, source), 0, palette.accent, publishAlpha * source * 0.9, { count: 1, speed: 3.2, phase: 0.6 });
    const beat = visibilityWindow(progress, 0.2996, 0.3147, 0.0114);
    scene.panel(PUBLISH.x, PUBLISH.y, PUBLISH.w, PUBLISH.h, publishAlpha, 0.5 + beat * 0.5, palette.accent);
    scene.mono(11.5, 620);
    scene.text('PUBLISH', PUBLISH.x, PUBLISH.y, palette.ink, publishAlpha, 'center');
    const short = scene.viewport !== 'wide';
    const targets = [
      { title: 'SUBSCRIBERS', sub: short ? 'Convex WS · Listen' : 'Convex WS · Firestore Listen', y: -190 },
      { title: 'TRIGGERS', sub: short ? 'functions · runtime' : 'functions · back in the runtime', y: 0 },
      { title: 'SCHEDULER', sub: 'runAfter · durable', y: 190 },
    ];
    const fan = smoothstep(range(progress, 0.3071, 0.3265));
    let railLeft = FANOUT_X;
    let railRight = FANOUT_X;
    targets.forEach((target, index) => {
      const reveal = clamp(fan * 1.6 - index * 0.3);
      // A ripple runs down each lane as the change reaches its target.
      const ripple = visibilityWindow(progress, 0.3651 + index * 0.0057, 0.3774 + index * 0.0057, 0.0057);
      // The card grows to hold its text where the text floor makes it wide.
      scene.mono(10.5);
      const cardW = Math.max(230, scene.measure(target.sub) + scene.px(32));
      const cardLeft = FANOUT_X - cardW / 2;
      scene.lane(PUBLISH.x + PUBLISH.w / 2, 0, cardLeft, target.y * reveal, palette.accent, publishAlpha * reveal * (0.5 + ripple * 0.5), 1.2 + ripple);
      scene.laneTraffic(PUBLISH.x + PUBLISH.w / 2, 0, cardLeft, target.y * reveal, palette.accent, publishAlpha * reveal * 0.9, { count: index === 0 ? 2 : 1, speed: index === 0 ? 3.2 : 2.2, phase: 0.3 + index * 0.31 });
      scene.panel(FANOUT_X, target.y * reveal, cardW, 58, publishAlpha * reveal, 0.2 + ripple * 0.5, palette.accent);
      scene.mono(11, 600);
      scene.text(target.title, cardLeft + 15, target.y * reveal - 10, palette.ink, publishAlpha * reveal);
      scene.mono(10.5);
      scene.text(target.sub, cardLeft + 15, target.y * reveal + 12, palette.muted, publishAlpha * reveal);
      if (index === targets.length - 1) {
        railLeft = cardLeft + 15;
        railRight = cardLeft + cardW - 15;
      }
    });

    // The schedule's promise, under its card: a marker rides the rail past a
    // restart tick and still arrives. It runs once the fan-out has settled.
    const railAlpha = publishAlpha * clamp(fan * 1.6 - 0.6);
    const railY = targets[2].y + 29 + scene.px(34);
    const slide = smoothstep(range(progress, 0.3331, 0.3433));
    scene.line(railLeft, railY, railRight, railY, palette.ink, railAlpha * 0.2);
    const restartX = mix(railLeft, railRight, 0.55);
    scene.line(restartX, railY - scene.px(8), restartX, railY + scene.px(8), palette.muted, railAlpha * 0.6, 1, [3, 3]);
    scene.mono(10);
    scene.text('restart', restartX, railY - scene.px(18), palette.muted, railAlpha * 0.8, 'center');
    ctx.save();
    ctx.translate(mix(railLeft + scene.px(4), railRight - scene.px(4), slide), railY);
    ctx.rotate(Math.PI / 4);
    const m = scene.px(4);
    ctx.fillStyle = rgba(palette.accent, railAlpha * (0.25 + scene.detail * 0.75));
    ctx.fillRect(-m, -m, m * 2, m * 2);
    ctx.restore();
    scene.text('at-least-once', railRight, railY + scene.px(18), palette.accentText, railAlpha * slide, 'right');
  }
  scene.textAlpha = 1;
}

// Storage backends: the shelf beneath the transaction. The commit drops onto
// the default engine, and the other engines sit beside it as the same storage
// layer with a different flag. The KV and the byte plane sit on the row below.
// One engine tile: the name, the tenant model as a chip, and one line on
// where its bytes live. Titles stay until the camera is far out; the rest
// needs the detail band.
function drawEngineTile(scene: Scene, tile: { name: string; chip: string; sub: string }, x: number, y: number, alpha: number, lit: number) {
  const { palette } = scene;
  const { far, richText } = scene;
  scene.cylinder(x, y, ENGINE_TILE.w, ENGINE_TILE.h, alpha, 0.2 + lit * 0.5, palette.accent);
  const left = x - ENGINE_TILE.w / 2 + 16;
  const bodyTop = scene.cylinderBodyTop(y, ENGINE_TILE.w, ENGINE_TILE.h);
  scene.mono(11.5, 620);
  scene.text(tile.name, left, bodyTop + scene.px(15), mixRgb(palette.ink, palette.accentText, lit), alpha * far);
  scene.chip(tile.chip, left, bodyTop + scene.px(31), alpha * richText, lit, palette.accent, 'left', ENGINE_TILE.w - 32);
  scene.mono(10.5);
  scene.text(tile.sub, left, bodyTop + scene.px(47), palette.muted, alpha * richText);
}

// The process edge, before the whole outline is drawn in the binary chapter:
// a dashed line where a lane leaves the process for a server the operator
// runs. It fades as the real outline arrives.
function drawEdgeHint(scene: Scene, progress: number, x0: number, x1: number, alpha: number, label: string, text = 1) {
  const outline = visibilityWindow(progress, 0.6921, 0.8432, 0.038);
  const show = alpha * (1 - outline);
  if (show <= 0.01) return;
  const { palette } = scene;
  scene.line(x0, BINARY.y1, x1, BINARY.y1, palette.accent, show * 0.45, 1.2, [8, 8]);
  // The label sits just inside the edge and a step in from its start, so it
  // never meets a title below it or the copy column beside it.
  scene.tag(label, x0 + scene.px(24), BINARY.y1 - scene.px(14), show * scene.richText * text, palette.muted);
}

function drawBackends(scene: Scene, progress: number) {
  const alpha = stage(progress, 0.3399, 0.4045, 0.0114);
  const bottom = ENGINE_TILE.hostedY + ENGINE_TILE.h / 2;
  if (alpha <= 0.01 || !scene.inView(SHELF.left - 100, SHELF.right + 200, 200, 560, bottom + 40)) return;
  const { ctx, palette } = scene;
  const { far, richText } = scene;
  const reveal = smoothstep(range(progress, 0.3449, 0.3602));
  const planesTop = PLANES.y - PLANES.h / 2;
  const planesBottom = PLANES.y + PLANES.h / 2;
  const barTop = SHELF.y - SHELF.h / 2;
  const barBottom = SHELF.y + SHELF.h / 2;
  const embeddedTop = ENGINE_TILE.embeddedY - ENGINE_TILE.h / 2;
  const hostedTop = ENGINE_TILE.hostedY - ENGINE_TILE.h / 2;

  // The drop: from the sealed transaction straight down onto the documents
  // plane, ahead of the request that takes it.
  const drop = smoothstep(range(progress, 0.3433, 0.3526));
  const dropAlpha = alpha * drop;
  // On a phone the copy column sits over the transaction, so the drop
  // starts just above the planes.
  const dropTop = scene.viewport === 'compact' ? planesTop - 150 : TXN.h / 2 + 20 + 14 + 8;
  scene.line(TXN.x, dropTop, TXN.x, mix(dropTop, planesTop, drop), palette.accent, dropAlpha * 0.6, 1.6);
  // Commits keep landing on the shelf.
  scene.lineTraffic(TXN.x, dropTop, TXN.x, mix(dropTop, planesTop, drop), palette.accent, dropAlpha * 0.9, { count: 2, speed: 2.4 });
  // The tag sits just above the heading row, whatever height the drop starts at.
  scene.tag('commit written here', TXN.x + scene.px(12), planesTop - 78, dropAlpha * richText, palette.accentText);

  // Through the layer into the default engine, as the commit lands.
  const lit = smoothstep(range(progress, 0.3552, 0.3654));
  scene.line(TXN.x, planesBottom, TXN.x, mix(planesBottom, barTop, clamp(lit * 2)), palette.accent, alpha * lit * 0.6, 1.6);
  scene.line(TXN.x, barBottom, TXN.x, mix(barBottom, embeddedTop, clamp(lit * 2 - 1)), palette.accent, alpha * lit * 0.6, 1.6);
  scene.lineTraffic(TXN.x, planesBottom, TXN.x, mix(barBottom, embeddedTop, clamp(lit * 2 - 1)), palette.accent, alpha * clamp(lit * 2 - 1) * 0.9, { count: 2, speed: 2.4, phase: 0.5 });

  // Heading: one storage layer, and the engine is a deployment decision.
  scene.mono(11, 620);
  scene.text('DURABLE STATE', SHELF.left, 590, palette.ink, alpha * far * reveal);
  const headW = scene.measure('DURABLE STATE');
  scene.tag('one storage layer · engine selected at deployment', SHELF.left + headW + scene.px(12), 590, alpha * richText * reveal);

  // Row one: what sits on the layer. Documents light as the commit lands.
  planeTiles.forEach((tile, index) => {
    const show = clamp(reveal * 1.8 - index * 0.3);
    const glow = index === 0 ? lit : 0;
    const w = tile.x1 - tile.x0;
    scene.panel((tile.x0 + tile.x1) / 2, PLANES.y, w, PLANES.h, alpha * show, tile.emphasis * 0.4 + glow * 0.5, palette.accent);
    scene.mono(11.5, 620);
    scene.text(tile.title, tile.x0 + 16, PLANES.y - 12, mixRgb(palette.ink, palette.accentText, glow), alpha * show * far);
    const titleW = scene.measure(tile.title);
    scene.chip(tile.chip, tile.x0 + 16 + titleW + scene.px(12), PLANES.y - 12, alpha * show * richText, tile.emphasis, palette.accent, 'left', w - 32 - titleW - scene.px(12));
    scene.mono(10.5);
    scene.text(tile.sub, tile.x0 + 16, PLANES.y + 14, palette.muted, alpha * show * richText);
  });

  // Row two: the storage layer, one seam over every engine.
  const layer = smoothstep(range(progress, 0.3565, 0.3683));
  scene.panel((SHELF.left + SHELF.right) / 2, SHELF.y, SHELF.right - SHELF.left, SHELF.h, alpha * layer, 0.3 + lit * 0.3, palette.accent);
  scene.mono(11.5, 620);
  scene.text('STORAGE LAYER', SHELF.left + 16, SHELF.y + 4, mixRgb(palette.ink, palette.accentText, lit * 0.6), alpha * layer * far);
  const barW = scene.measure('STORAGE LAYER');
  scene.chip('nimbus-storage', SHELF.left + 16 + barW + scene.px(12), SHELF.y + 4, alpha * layer * richText, 0, palette.accent);
  scene.tag('one seam · engine set by a flag on nimbus start', SHELF.right - 16, SHELF.y + 4, alpha * layer * richText, palette.muted, 'right');

  // Row three, inside the process: the embedded engines, libraries that write
  // under the data directory. The default lights up as the commit lands.
  const engines = smoothstep(range(progress, 0.3608, 0.376));
  embeddedEngines.forEach((tile, index) => {
    const show = clamp(engines * 1.6 - index * 0.3);
    const glow = index === 0 ? lit : 0;
    scene.line(tile.x, barBottom, tile.x, mix(barBottom, embeddedTop, show), palette.accent, alpha * show * (index === 0 ? 0 : 0.5), 1.2);
    drawEngineTile(scene, tile, tile.x, ENGINE_TILE.embeddedY, alpha * show, glow);
  });

  // Row four, below the process edge: the hosted engines, servers the
  // operator runs. Each lane is a connection that leaves the process.
  // The hosted row lands with the embedded row, so the chapter still shows
  // every engine.
  const hosted = smoothstep(range(progress, 0.3608, 0.3743));
  hostedEngines.forEach((tile, index) => {
    const show = clamp(hosted * 1.6 - index * 0.3);
    scene.line(tile.x, barBottom, tile.x, mix(barBottom, hostedTop, show), palette.muted, alpha * show * 0.55, 1.2, [6, 8]);
    // A hosted engine is a connection that stays open.
    scene.lineTraffic(tile.x, barBottom, tile.x, mix(barBottom, hostedTop, show), palette.muted, alpha * show * 0.8, { count: 1, speed: 1.2 + index * 0.2, phase: index * 0.33 });
    drawEngineTile(scene, tile, tile.x, ENGINE_TILE.hostedY, alpha * show, 0);
  });
  ctx.setLineDash([]);
}

// The files panel: layers laid out from the top in screen pixels, so the
// labels fit at any zoom and the panel grows to hold them.
function filesLayout(scene: Scene) {
  // One screen pixel, capped so the panel stays a panel when the camera is
  // far back and the text has left.
  const unit = Math.min(scene.px(1), 2);
  const pad = unit * 26;
  const gap = unit * 30;
  const headY = FILES.top + pad;
  const cardH = mix(unit * 40, unit * 56, scene.rich);
  // The top row carries two lines: the volume card says where a snapshot
  // goes and what a fork makes.
  const rowH = mix(unit * 40, unit * 74, scene.rich);
  const byteH = mix(unit * 40, unit * 74, scene.rich);
  const rowTop = headY + unit * 32;
  const objectsTop = rowTop + rowH + gap;
  const byteTop = objectsTop + cardH + gap;
  const bottom = byteTop + byteH + pad;
  return { headY, rowTop, objectsTop, byteTop, cardH, rowH, byteH, bottom };
}

// Files and volumes: one byte plane, built up. Bytes land in the
// content-addressed plane; objects and their S3 API sit on it; the isolate
// filesystem and the named volume the box mounts sit on that. The request
// arrives from the shelf, so no lane back to it is drawn. The volume's lane
// in the box chapter starts at the VOLUMES card, after the panel has gone.
function drawFiles(scene: Scene, progress: number) {
  const alpha = stage(progress, 0.3965, 0.4657, 0.0285);
  const left = FILES.x - FILES.w / 2;
  const right = FILES.x + FILES.w / 2;
  if (alpha <= 0.01 || !scene.inView(SHELF.right, right, 200, FILES.top - 100, OBJECT_STORE.y + OBJECT_STORE.h / 2 + 40)) return;
  const { palette } = scene;
  const { far, richText } = scene;
  const layout = filesLayout(scene);
  const byte = smoothstep(range(progress, 0.4018, 0.4095));
  const objects = smoothstep(range(progress, 0.4061, 0.4147));
  const top = smoothstep(range(progress, 0.4113, 0.4198));
  const innerLeft = FILES.x - FILES.inner / 2;

  scene.panel(FILES.x, (FILES.top + layout.bottom) / 2, FILES.w, layout.bottom - FILES.top, alpha, 0.1, palette.accent);
  scene.mono(11, 620);
  scene.text('FILES AND VOLUMES', innerLeft, layout.headY, palette.ink, alpha * far);
  const headW = scene.measure('FILES AND VOLUMES');
  scene.tag('one byte plane · layered', innerLeft + headW + scene.px(12), layout.headY, alpha * richText);

  const card = (cx: number, cardTop: number, w: number, h: number, show: number, lit: number, title: string, chip: string, lines: string[]) => {
    scene.panel(cx, cardTop + h / 2, w, h, alpha * show, 0.2 + lit * 0.5, palette.accent);
    const x = cx - w / 2 + 16;
    scene.mono(11.5, 620);
    scene.text(title, x, cardTop + scene.px(20), mixRgb(palette.ink, palette.accentText, lit), alpha * show * far);
    const titleW = scene.measure(title);
    scene.chip(chip, x + titleW + scene.px(12), cardTop + scene.px(20), alpha * show * richText, lit, palette.accent, 'left', 600);
    scene.mono(10.5);
    lines.forEach((line, index) => {
      scene.text(line, x, cardTop + scene.px(38 + 18 * index), palette.muted, alpha * show * richText);
    });
  };

  // Bottom up: the byte plane lights first, since everything rests on it.
  card(FILES.x, layout.byteTop, FILES.inner, layout.byteH, byte, byte, 'BYTE PLANE', 'nimbus-blob', [
    'BLAKE3 · content-addressed · one store per tenant',
    'encrypted below placement · per-tenant key',
  ]);
  card(FILES.x, layout.objectsTop, FILES.inner, layout.cardH, objects, 0, 'OBJECTS', 'nimbus-s3 · S3 API :9000', ['manifests commit through storage · Convex _storage']);
  card(FILES_CARD.xs[0], layout.rowTop, FILES_CARD.w, layout.rowH, top, 0, 'FILES', 'nimbus-fs', ['POSIX FS · in the isolate']);
  card(FILES_CARD.xs[1], layout.rowTop, FILES_CARD.w, layout.rowH, clamp(top * 1.4 - 0.4), 0, 'VOLUMES', 'scratch · per tenant', ['host directory · mounted at /work', 'snapshot to blob · no API yet']);

  // Built on: lanes rise from each layer into the one above it.
  const objectsBottom = layout.objectsTop + layout.cardH;
  const rowBottom = layout.rowTop + layout.rowH;
  scene.line(FILES.x, layout.byteTop, FILES.x, mix(layout.byteTop, objectsBottom, objects), palette.accent, alpha * objects * 0.6, 1.4);
  // Bytes move both ways between the planes.
  scene.lineTraffic(FILES.x, layout.byteTop, FILES.x, mix(layout.byteTop, objectsBottom, objects), palette.accent, alpha * objects * 0.9, { count: 2, speed: 2.4, reverse: true });
  scene.tag('built on', FILES.x + scene.px(10), (layout.byteTop + objectsBottom) / 2, alpha * objects * richText);
  FILES_CARD.xs.forEach((x, index) => {
    scene.line(x, layout.objectsTop, x, mix(layout.objectsTop, rowBottom, top), palette.accent, alpha * top * 0.6, 1.4);
    scene.lineTraffic(x, layout.objectsTop, x, mix(layout.objectsTop, rowBottom, top), palette.accent, alpha * top * 0.9, { count: 1, speed: 2.4, phase: 0.5 + index * 0.4, reverse: index === 0 });
  });

  // Below the process edge: the bucket the operator owns. Placement mirrors
  // or tiers the bytes there, or makes it primary; the local pack is the
  // default and needs no bucket.
  const cloud = smoothstep(range(progress, 0.4164, 0.4248));
  const storeTop = OBJECT_STORE.y - OBJECT_STORE.h / 2;
  scene.line(FILES.x, layout.bottom, FILES.x, mix(layout.bottom, storeTop, cloud), palette.muted, alpha * cloud * 0.55, 1.2, [6, 8]);
  scene.lineTraffic(FILES.x, layout.bottom, FILES.x, mix(layout.bottom, storeTop, cloud), palette.muted, alpha * cloud * 0.8, { count: 1, speed: 1.2, phase: 0.7 });
  scene.tag('placement · optional', FILES.x + scene.px(10), (layout.bottom + BINARY.y1) / 2, alpha * cloud * richText, palette.muted);
  scene.cylinder(OBJECT_STORE.x, OBJECT_STORE.y, OBJECT_STORE.w, OBJECT_STORE.h, alpha * cloud, 0.2, palette.accent);
  const storeLeft = OBJECT_STORE.x - OBJECT_STORE.w / 2 + 16;
  const storeBody = scene.cylinderBodyTop(OBJECT_STORE.y, OBJECT_STORE.w, OBJECT_STORE.h);
  scene.mono(11.5, 620);
  scene.text('OBJECT STORE', storeLeft, storeBody + scene.px(15), palette.ink, alpha * cloud * far);
  const storeW = scene.measure('OBJECT STORE');
  scene.chip('S3 · GCS', storeLeft + storeW + scene.px(12), storeBody + scene.px(15), alpha * cloud * richText, 0, palette.accent);
  scene.mono(10.5);
  scene.text('an external bucket · mirror · tier · cloud-primary', storeLeft, storeBody + scene.px(33), palette.muted, alpha * cloud * richText);
  drawEdgeHint(scene, progress, left, right, alpha * cloud, 'nimbus process above · external bucket below');
}

function drawSandbox(scene: Scene, progress: number) {
  const full = visibilityWindow(progress, 0.4291, 0.7983, 0.0333);
  // Chapters 10 to 12 build on the box. Once its own chapter has passed, the
  // box, its host and everything found on them step back to outlines and
  // their detail text leaves, so the primitive each later chapter adds is
  // the only thing at full contrast. The egress path steps back the same way
  // once its chapter has passed. Everything returns for the pullback that
  // frames the whole process.
  const past = smoothstep(range(progress, 0.495, 0.5056)) * (1 - smoothstep(range(progress, 0.6972, 0.7078)));
  const egressPast = smoothstep(range(progress, 0.5455, 0.5561)) * (1 - smoothstep(range(progress, 0.6972, 0.7078)));
  const alpha = full * (1 - past * 0.62);
  const proxyAlpha = full * (1 - egressPast * 0.62);
  const boxText = scene.richText * (1 - past);
  const proxyText = scene.richText * (1 - egressPast);
  const hostLeft = HOST.x - HOST.w / 2;
  const hostRight = HOST.x + HOST.w / 2;
  const hostTop = HOST.y - HOST.h / 2;
  const hostBottom = HOST.y + HOST.h / 2;
  if (full <= 0.01 || !scene.inView(VOLUME_LANE_X - 200, BINARY.x1 + 100, 120, INGRESS.y - 220, hostBottom + 80)) return;
  const { ctx, palette } = scene;
  const { far, rich } = scene;
  const left = SANDBOX.x - SANDBOX.w / 2;
  const right = SANDBOX.x + SANDBOX.w / 2;
  // At label level the box top drops a little so the host title, which is
  // screen-sized, keeps clear of its edge.
  const boxTop = SANDBOX.y - SANDBOX.h / 2;
  const top = boxTop + mix(scene.px(8), 0, rich);
  const bottom = boxTop + mix(SANDBOX.h - 60, SANDBOX.h, rich);
  const boxH = bottom - top;
  const egressY = top + mix(50, 80, rich);
  // Chapter 09 in the order nimbus works: the ground and the tenant door, the
  // image pipeline, the wall, then the box and the parts that land in it.
  const ground = smoothstep(range(progress, 0.4376, 0.4546));
  const door = smoothstep(range(progress, 0.4504, 0.4657));
  const admitted = visibilityWindow(progress, 0.4665, 0.7983, 0.0095);
  const image = smoothstep(range(progress, 0.447, 0.4673));
  const wall = smoothstep(range(progress, 0.4588, 0.4741));
  const draw = smoothstep(range(progress, 0.4673, 0.4793));
  const root = smoothstep(range(progress, 0.4724, 0.4827));
  const volume = smoothstep(range(progress, 0.4759, 0.4877));
  const agent = smoothstep(range(progress, 0.481, 0.4894));
  const agentLit = smoothstep(range(progress, 0.4827, 0.4911));
  const facts = smoothstep(range(progress, 0.4776, 0.4894));
  // Chapter 10: the gate, the proxy, its rules, then the two verdicts.
  // Chapter 13 owns the frame with the session stack: the egress path steps
  // out for it and returns on the way to the outline.
  const proxyOut = 1 - visibilityWindow(progress, 0.6016, 0.7078, 0.0106);
  const egress = smoothstep(range(progress, 0.5013, 0.514)) * proxyOut;
  const proxy = smoothstep(range(progress, 0.5099, 0.5226)) * proxyOut;
  const rules = smoothstep(range(progress, 0.5183, 0.5311)) * proxyOut;
  const allowed = smoothstep(range(progress, 0.5234, 0.5328)) * proxyOut;
  const denied = smoothstep(range(progress, 0.5269, 0.5353)) * proxyOut;

  // The ground: the host, a boundary in ink below the process edge. It is
  // the machine nimbus runs on, so it is drawn as a thing found, not made.
  const hostPerimeter = 2 * (HOST.w + HOST.h);
  scene.roundRect(hostLeft, hostTop, HOST.w, HOST.h, 18);
  ctx.fillStyle = rgba(mixRgb(palette.background, palette.ink, 0.03), alpha * ground);
  ctx.fill();
  ctx.lineWidth = scene.px(1.4);
  ctx.strokeStyle = rgba(palette.ink, alpha * ground * 0.5);
  ctx.setLineDash([hostPerimeter * ground, hostPerimeter]);
  ctx.stroke();
  ctx.setLineDash([]);
  scene.mono(12, 620);
  // Titles keep a readable distance from their edge at every zoom.
  const titleDrop = Math.max(22, scene.px(15));
  scene.text('HOST', hostLeft + 22, hostTop + titleDrop, palette.ink, alpha * ground * far);
  const hostW = scene.measure('HOST');
  scene.tag('host machine · Linux · macOS via VM', hostLeft + 22 + hostW + scene.px(12), hostTop + titleDrop, alpha * ground * boxText);
  drawEdgeHint(scene, progress, hostLeft, BINARY.x1, alpha * ground, 'nimbus process above · the host below', 1 - past);

  // The tenant door, inside the process above the box: the control plane is
  // the Nimbus API in the process. It admits the SDK call against the
  // tenant's quota, keeps the records for services, sandboxes and sessions,
  // and creates and leases the box below from the compose file. The card
  // hangs from a fixed top edge and grows with its rows, which are
  // screen-sized like the text they hold, so the chip never rides its line.
  // A lane drops from the card across the process edge into the box top,
  // so the card reads as the thing that made the box, not a label near it.
  const doorX = CONTROL.x - CONTROL.w / 2;
  const doorTop = CONTROL.top;
  // The card stays inside the process: where a fitted still pulls the zoom
  // back, the card stops at the edge and its last row yields.
  const doorRoom = BINARY.y1 - 12 - doorTop;
  const doorH = Math.min(mix(scene.px(38), scene.px(142), rich), doorRoom);
  const doorBottom = doorTop + doorH;
  scene.panel(CONTROL.x, doorTop + doorH / 2, CONTROL.w, doorH, alpha * door, 0.3 + admitted * 0.4, palette.accent);
  scene.mono(11.5, 620);
  scene.text('CONTROL PLANE', doorX + 16, doorTop + scene.px(20), palette.ink, alpha * door * far);
  scene.mono(10.5);
  scene.text('POST /api/sessions', doorX + 16, doorTop + scene.px(40), palette.muted, alpha * door * boxText);
  scene.chip('admitted · tenant quota', doorX + 16, doorTop + scene.px(63), alpha * door * boxText, admitted, palette.accent, 'left', CONTROL.w - 32);
  scene.mono(10.5);
  scene.text('registry: services ·', doorX + 16, doorTop + scene.px(88), palette.muted, alpha * door * boxText);
  scene.text('sandboxes · sessions', doorX + 16, doorTop + scene.px(106), palette.muted, alpha * door * boxText);
  if (scene.px(132) <= doorH) {
    scene.text('creates the box below', doorX + 16, doorTop + scene.px(124), palette.accentText, alpha * door * boxText * 0.9);
  }
  // The lane from the card to the box it creates: down the card's centre
  // line, across the edge, onto the box top. It lights with the box.
  const creates = smoothstep(range(progress, 0.4665, 0.4793)) * (1 - past);
  scene.line(CONTROL.x, doorBottom, CONTROL.x, mix(doorBottom, boxTop, creates), palette.accent, alpha * creates * 0.55, 1.4, [6, 8]);

  // The image pipeline on the ground under the box: a reference in, a root
  // filesystem out, and no daemon between. The root rises into the box floor.
  // Titles are screen-sized while the cards are world-sized, so the label
  // level uses a shorter title and the two cross-fade with the detail.
  const cards = [
    { title: 'OCI IMAGE', short: 'OCI IMAGE', sub: 'python:3.12 by ref' },
    { title: 'PULL · UNPACK', short: 'UNPACK', sub: 'unpack · no daemon' },
    { title: 'ROOT FS', short: 'ROOT FS', sub: 'sha256 receipt' },
  ];
  const cardH = mix(PIPELINE.h, scene.px(42), rich);
  // The rows on the ground step out with the egress path for the session
  // stack, which reaches the box from above, and return for the pullback.
  const groundOut = proxyOut;
  cards.forEach((card, index) => {
    const show = clamp(image * 1.6 - index * 0.3) * groundOut;
    const cx = PIPELINE.xs[index];
    scene.panel(cx, PIPELINE.y, PIPELINE.w, cardH, alpha * show, index === 2 ? 0.5 : 0.2, palette.accent);
    scene.mono(11.5, 620);
    const titleY = PIPELINE.y - mix(0, scene.px(9), rich);
    if (card.short !== card.title) {
      scene.text(card.short, cx - PIPELINE.w / 2 + 14, titleY, palette.ink, alpha * show * far * (1 - rich));
    }
    scene.text(card.title, cx - PIPELINE.w / 2 + 14, titleY, palette.ink, alpha * show * far * (card.short === card.title ? 1 : rich));
    scene.mono(10.5);
    scene.text(card.sub, cx - PIPELINE.w / 2 + 14, PIPELINE.y + scene.px(10), palette.muted, alpha * show * boxText);
    if (index < cards.length - 1) {
      const x0 = cx + PIPELINE.w / 2;
      const x1 = PIPELINE.xs[index + 1] - PIPELINE.w / 2;
      scene.lane(x0, PIPELINE.y, mix(x0, x1, show), PIPELINE.y, palette.accent, alpha * show * 0.6, 1.4);
      scene.lineTraffic(x0, PIPELINE.y, mix(x0, x1, show), PIPELINE.y, palette.accent, alpha * show * 0.9, { count: 1, speed: 2.6, phase: index * 0.5 });
    }
  });
  scene.tag('pin by digest · optional', hostRight - 22, PIPELINE.y, alpha * image * groundOut * boxText, palette.muted, 'right');

  // The wall: how one host is split. Two landed kinds stand beside the box.
  // The microVM lights: it is the wall compose picked with backend: krun, and
  // it reaches the box, whose outline is drawn in its colour.
  const kinds = [
    { title: 'CONTAINER · crun', short: 'CONTAINER', lines: ['OCI bundle · shared kernel', 'cgroup v2 cpu quota · memory'], limit: 'cpus 2 · memory 1G' },
    { title: 'MICROVM · libkrun', short: 'MICROVM', lines: ['own kernel · own memory map', 'krun cpus · memory limit'], limit: 'cpus 1 · memory 512M' },
  ];
  // The rows are screen-sized text, so at detail level the cards keep a
  // screen-sized minimum width and grow leftward from a fixed right edge.
  const wallRight = WALL.x + WALL.w / 2;
  const wallW = mix(WALL.w, Math.max(WALL.w, scene.px(200)), rich);
  const kindRows = [22 + scene.px(20), scene.px(15), scene.px(22)];
  const kindFullH = kindRows.reduce((sum, row) => sum + row, 0) + scene.px(18);
  const kindH = mix(52, kindFullH, rich);
  const wallX = wallRight - wallW;
  scene.tag('isolation · set per sandbox', hostRight - 22, hostTop + titleDrop, alpha * wall * groundOut * boxText, palette.accentText, 'right');
  let kindTop = WALL.top;
  kinds.forEach((kind, index) => {
    const show = clamp(wall * 1.4 - index * 0.4) * groundOut;
    const chosen = index === 1;
    scene.panel(wallRight - wallW / 2, kindTop + kindH / 2, wallW, kindH, alpha * show, chosen ? 0.7 : 0.25, palette.accent);
    scene.mono(11.5, 620);
    const kindInk = chosen ? palette.accent : palette.ink;
    scene.text(kind.short, wallX + 16, kindTop + 22, kindInk, alpha * show * far * (1 - rich));
    scene.text(kind.title, wallX + 16, kindTop + 22, kindInk, alpha * show * far * rich);
    scene.mono(10.5);
    let lineY = kindTop + kindRows[0];
    kind.lines.forEach((line, row) => {
      scene.text(line, wallX + 16, lineY, palette.muted, alpha * show * boxText);
      lineY += kindRows[row + 1];
    });
    scene.chip(kind.limit, wallX + 16, lineY, alpha * show * boxText, chosen ? 1 : 0.15, palette.accent, 'left', wallW - 32);
    if (chosen) {
      const laneY = kindTop + kindH / 2;
      scene.lane(wallX, laneY, mix(wallX, right + 8, draw), laneY, palette.accent, alpha * draw * 0.7, 1.4);
      scene.laneTraffic(wallX, laneY, mix(wallX, right + 8, draw), laneY, palette.accent, alpha * draw * 0.9, { count: 1, speed: 2.4, reverse: true });
    }
    kindTop += kindH + 30;
  });
  // The reserved kinds are a footnote: shown only where a line of room is
  // left between the wall and the image pipeline.
  const reservedRoom = clamp((PIPELINE.y - cardH / 2 - (kindTop + scene.px(12))) / scene.px(8));
  scene.tag('isolate · wasm · reserved · not implemented', hostRight - 22, kindTop + scene.px(6), alpha * facts * groundOut * boxText * reservedRoom, palette.muted, 'right');

  // The box, drawn as one continuous outline in the wall's colour: a
  // boundary, not a card.
  const perimeter = 2 * (SANDBOX.w + boxH);
  scene.roundRect(left, top, SANDBOX.w, boxH, 16);
  ctx.fillStyle = rgba(mixRgb(palette.background, palette.accent, 0.06), alpha * draw);
  ctx.fill();
  ctx.lineWidth = scene.px(1.6);
  ctx.strokeStyle = rgba(palette.accent, alpha * 0.8);
  ctx.setLineDash([perimeter * draw, perimeter]);
  ctx.stroke();
  ctx.setLineDash([]);
  scene.mono(12, 620);
  scene.text('SANDBOX', left + 22, top + titleDrop, palette.ink, alpha * draw * far);
  const titleW = scene.measure('SANDBOX');
  // The wall the box uses is named by the lit card in the wall column, so
  // the title row carries only when the box was carved.
  scene.tag('like a pod · created with the service', left + 22 + titleW + scene.px(12), top + titleDrop, alpha * draw * boxText);

  // Inside, the parts land in the order nimbus builds them: the root
  // filesystem rises from the pipeline into the floor, the volume arrives
  // along its lane from the files plane, and the agent process lands on top
  // and lights. On a phone the cards use the full width, since the column of
  // facts beside them is hidden.
  const cardW = mix(SANDBOX.w - 44, AGENT.w, rich);
  const cardX = left + 22 + cardW / 2;
  // The floor bar is box detail: at the label level the pipeline's own ROOT FS
  // card names the root and its lane simply enters the box.
  const floorY = bottom - 22 - ROOTFS.h / 2;
  const rootX = PIPELINE.xs[2] - 45;
  const laneEnd = mix(bottom, floorY + ROOTFS.h / 2, rich);
  scene.line(rootX, PIPELINE.y - cardH / 2, rootX, mix(PIPELINE.y - cardH / 2, laneEnd, root), palette.accent, alpha * root * groundOut * 0.7, 1.6);
  scene.lineTraffic(rootX, PIPELINE.y - cardH / 2, rootX, mix(PIPELINE.y - cardH / 2, laneEnd, root), palette.accent, alpha * root * groundOut * 0.9, { count: 1, speed: 2.2, phase: 0.6, reverse: true });
  scene.panel(ROOTFS.x, floorY, ROOTFS.w, ROOTFS.h, alpha * root * rich, 0.3, palette.accent);
  scene.mono(11.5, 620);
  // The chip is screen-sized, so the two rows are spaced in screen pixels.
  scene.text('ROOT FS', left + 36, floorY - scene.px(8), palette.ink, alpha * root * rich);
  const rootW = scene.measure('ROOT FS');
  scene.chip('unpacked below', left + 36 + rootW + scene.px(12), floorY - scene.px(8), alpha * root * boxText, 0.3, palette.accent);
  scene.mono(10.5);
  scene.text('python:3.12 · unpacked on the host', left + 36, floorY + scene.px(12), palette.muted, alpha * root * boxText);
  // At the label level the part cards are screen-sized, so they stack in
  // screen pixels under the title instead of at their composed world rows.
  // Far out, where the titles have already left, screen-sized cards would
  // spill under the host, so they return to their composed rows.
  const labelPartH = mix(AGENT.h, scene.px(26), far);
  const partH = mix(labelPartH, AGENT.h, rich);
  const labelAgentY = mix(AGENT.y, top + titleDrop + scene.px(12) + partH / 2, far);
  const agentY = mix(labelAgentY, AGENT.y, rich);
  const volumeY = mix(mix(VOLUME.y, agentY + partH + scene.px(8), far), VOLUME.y, rich);
  const parts = [
    { y: volumeY, title: 'VOLUME', sub: 'scratch:/work', from: 'chapter 08', show: volume, dx: -60, dy: 0, lit: 0 },
    { y: agentY, title: 'AGENT PROCESS', sub: 'python agent.py', from: '', show: agent, dx: 0, dy: -44, lit: agentLit },
  ];
  parts.forEach((part) => {
    const slide = 1 - part.show;
    const ox = part.dx * slide;
    const oy = part.dy * slide;
    const y = part.y + oy;
    scene.panel(cardX + ox, y, cardW, partH, alpha * part.show, 0.2 + part.lit * 0.6, palette.accent);
    scene.mono(11.5, 620);
    scene.text(part.title, left + 36 + ox, y - 12 * rich, mixRgb(palette.ink, palette.accentText, part.lit), alpha * part.show * far);
    if (part.from) {
      scene.chip(part.from, cardX + ox + cardW / 2 - 14, y - 12 * rich, alpha * part.show * boxText, 0.3, palette.accent, 'right');
    }
    scene.mono(10.5);
    scene.text(part.sub, left + 36 + ox, y + 14, palette.muted, alpha * part.show * boxText);
  });

  // The facts beside the parts: whose box it is, how it is addressed, what
  // it cannot do. Chips keep a screen-pixel height, so the column is laid
  // out in screen pixels.
  const column = alpha * facts * boxText;
  if (column > 0.01) {
    let y = AGENT.y - AGENT.h / 2 + 14;
    scene.tag('owner: service agent', SANDBOX_COLUMN_X, y, column, palette.ink);
    y += scene.px(22);
    scene.chip('id · for life', SANDBOX_COLUMN_X, y, column, 0.6, palette.accent, 'left', right - 14 - SANDBOX_COLUMN_X);
    y += scene.px(26);
    scene.tag('get · stop · by id', SANDBOX_COLUMN_X, y, column);
    y += scene.px(20);
    scene.tag('launch inputs redacted', SANDBOX_COLUMN_X, y, column);
    y += scene.px(20);
    scene.tag('no path back to ctx.db', SANDBOX_COLUMN_X, y, column, palette.accentText);
  }

  // The volume's lane: from the VOLUMES card in the files plane, down past
  // the process edge and in through the box's left wall. The files plane
  // named it; the box mounts it.
  const files = filesLayout(scene);
  const laneY0 = files.rowTop + files.rowH / 2;
  const laneX0 = FILES_CARD.xs[1] + FILES_CARD.w / 2;
  const seg1 = clamp(volume / 0.2);
  const seg2 = clamp((volume - 0.2) / 0.6);
  const seg3 = clamp((volume - 0.8) / 0.2);
  scene.line(laneX0, laneY0, mix(laneX0, VOLUME_LANE_X, seg1), laneY0, palette.accent, alpha * seg1 * 0.6, 1.4);
  scene.line(VOLUME_LANE_X, laneY0, VOLUME_LANE_X, mix(laneY0, volumeY, seg2), palette.accent, alpha * seg2 * 0.6, 1.4);
  // The volume's bytes move both ways.
  scene.lineTraffic(VOLUME_LANE_X, laneY0, VOLUME_LANE_X, mix(laneY0, volumeY, seg2), palette.accent, alpha * seg2 * 0.9, { count: 2, speed: 1.8, phase: 0.25 });
  scene.lineTraffic(VOLUME_LANE_X, laneY0, VOLUME_LANE_X, mix(laneY0, volumeY, seg2), palette.accent, alpha * seg2 * 0.9, { count: 1, speed: 1.8, phase: 0.6, reverse: true });
  scene.line(VOLUME_LANE_X, volumeY, mix(VOLUME_LANE_X, left, seg3), volumeY, palette.accent, alpha * seg3 * 0.6, 1.4);
  // The tag sits high on the lane, clear of the request that waits at the
  // tenant door beside the lane's lower half.
  scene.tag('named volume · mounted at /work', VOLUME_LANE_X + scene.px(10), mix(laneY0, BINARY.y1, 0.3), alpha * seg2 * boxText, palette.accentText);

  // Egress: a gate on the right wall, a lane up across the process edge into
  // the proxy that enforces the rules, and two ways out of the proxy: the one
  // host a rule names, and a cross for everything else.
  scene.roundRect(right - 8, egressY - 12, 16, 24, 4);
  ctx.fillStyle = rgba(palette.background, proxyAlpha * egress);
  ctx.fill();
  ctx.lineWidth = scene.px(1.2);
  ctx.strokeStyle = rgba(palette.accent, proxyAlpha * egress);
  ctx.stroke();
  // The rule text is screen-sized, so at detail level the proxy card keeps a
  // screen-sized minimum width and grows leftward from its exit edge.
  const px1 = PROXY.x + PROXY.w / 2;
  const proxyCardW = mix(PROXY.w, Math.max(PROXY.w, scene.px(330)), rich);
  const px0 = px1 - proxyCardW;
  const proxyRows = [24 + scene.px(24), scene.px(16), scene.px(16), scene.px(18)];
  const proxyFullH = proxyRows.reduce((sum, row) => sum + row, 0) + scene.px(18);
  const proxyH = mix(52, proxyFullH, rich);
  const py = PROXY.y - proxyH / 2;
  const across = clamp(egress * 2);
  const up = clamp(egress * 2 - 1);
  scene.line(right + 8, egressY, mix(right + 8, EGRESS.corner, across), egressY, palette.accent, proxyAlpha * across * 0.6, 1.2, [6, 8]);
  scene.line(EGRESS.corner, egressY, EGRESS.corner, mix(egressY, py + proxyH, up), palette.accent, proxyAlpha * up * 0.6, 1.2, [6, 8]);
  // The agent keeps asking out: every request goes up to the proxy.
  scene.lineTraffic(right + 8, egressY, mix(right + 8, EGRESS.corner, across), egressY, palette.accent, proxyAlpha * across * 0.9, { count: 1, speed: 2.6, phase: 0.2 });
  scene.lineTraffic(EGRESS.corner, egressY, EGRESS.corner, mix(egressY, py + proxyH, up), palette.accent, proxyAlpha * up * 0.9, { count: 1, speed: 2.6, phase: 0.7 });
  // The exit label needs a line of room between the box and the wall.
  const egressRoom = clamp((wallX - (right + 20) - scene.measure('EGRESS')) / scene.px(6));
  scene.tag('EGRESS', right + 20, egressY - scene.px(16), proxyAlpha * egress * proxyText * egressRoom, palette.accentText);

  scene.panel((px0 + px1) / 2, PROXY.y, proxyCardW, proxyH, proxyAlpha * proxy, 0.3, palette.accent);
  scene.mono(12, 620);
  scene.text('EGRESS PROXY', px0 + 20, py + 24, palette.ink, proxyAlpha * proxy * far);
  const proxyW = scene.measure('EGRESS PROXY');
  scene.chip('deny by default', px0 + 20 + proxyW + scene.px(12), py + 24, proxyAlpha * proxy * proxyText, 0.4, palette.accent);
  scene.mono(10.5);
  let proxyY = py + proxyRows[0];
  scene.text('rule stripe-api · https · api.stripe.com:443', px0 + 20, proxyY, palette.muted, proxyAlpha * rules * proxyText);
  proxyY += proxyRows[1];
  scene.text('POST · /v1/ · no wildcard hosts', px0 + 20, proxyY, palette.muted, proxyAlpha * rules * proxyText);
  proxyY += proxyRows[2];
  scene.text('splice by default · TLS only for credential/DLP', px0 + 20, proxyY, palette.muted, proxyAlpha * rules * proxyText);
  proxyY += proxyRows[3];
  scene.tag('the key stays outside the sandbox', px0 + 20, proxyY, proxyAlpha * rules * proxyText, palette.accentText);

  // Allowed: one lane out to the host the rule names. Its label sits under
  // the proxy with the denial, right-aligned to the exit, so the card keeps
  // a clean top edge and the pair reads as the two outcomes.
  const allowY = PROXY.y - scene.px(14);
  scene.line(px1, allowY, mix(px1, EGRESS.x1, allowed), allowY, palette.accent, proxyAlpha * allowed * 0.7, 1.4);
  scene.lineTraffic(px1, allowY, mix(px1, EGRESS.x1, allowed), allowY, palette.accent, proxyAlpha * allowed * 0.9, { count: 1, speed: 2.6, phase: 0.1 });
  ctx.beginPath();
  ctx.arc(EGRESS.x1, allowY, scene.px(4), 0, Math.PI * 2);
  ctx.fillStyle = rgba(palette.accent, proxyAlpha * allowed);
  ctx.fill();
  scene.chip('api.stripe.com:443 · POST /v1/', EGRESS.x1, py + proxyH + scene.px(20), proxyAlpha * allowed * proxyText, 1, palette.accent, 'right', proxyCardW);

  // Denied: everything else stops at the cross.
  const denyY = PROXY.y + scene.px(14);
  const crossX = EGRESS.x1 - 20;
  scene.line(px1, denyY, mix(px1, crossX - 16, denied), denyY, palette.accent, proxyAlpha * denied * 0.5, 1.2, [6, 8]);
  // Everything else runs to the cross and stops there.
  scene.lineTraffic(px1, denyY, mix(px1, crossX - 16, denied), denyY, palette.muted, proxyAlpha * denied * 0.9, { count: 1, speed: 2.6, phase: 0.55 });
  const cross = scene.px(6);
  scene.line(crossX - cross, denyY - cross, crossX + cross, denyY + cross, palette.accent, proxyAlpha * denied, 1.6);
  scene.line(crossX - cross, denyY + cross, crossX + cross, denyY - cross, palette.accent, proxyAlpha * denied, 1.6);
  scene.tag('all other traffic · denied', EGRESS.x1, py + proxyH + scene.px(44), proxyAlpha * denied * proxyText, palette.muted, 'right');

  // Ingress: the way in. A door in the right edge of the process, opposite
  // the protocol doors: a host TCP listener nimbus owns, leased for the
  // service. Bytes pass through it untouched, along a lane into the service
  // card, then from under the card down onto the guest port in the top of
  // the box: the service is the published endpoint, the box is where the
  // bytes land. The facts stand above the door, right-aligned to it. Like
  // the egress path, it steps out for the session chapter.
  const ingress = smoothstep(range(progress, 0.5625, 0.571)) * proxyOut;
  const ingressFacts = smoothstep(range(progress, 0.5659, 0.5745)) * proxyOut;
  const forward = smoothstep(range(progress, 0.5685, 0.5761)) * proxyOut;
  const ingressAlpha = full * ingress;
  // The door grows around its sub-row so the minimum screen font never runs
  // into the bar on its right edge. At the label level only the name shows,
  // so the door keeps a label's width and stays inside a tablet frame.
  scene.mono(11);
  const ingressW = Math.max(INGRESS.w, scene.px(84), mix(0, scene.measure('leased for the service') + 56, scene.richText));
  scene.mono(12, 560);
  const ingressLeft = INGRESS.x - ingressW / 2;
  const ingressRight = INGRESS.x + ingressW / 2;
  const ingressTop = INGRESS.y - DOOR_H / 2;
  // The edge beside the door, so the door reads as a door in it. The outline
  // chapter draws the whole edge, so the hint steps out for it.
  const edgeOutline = visibilityWindow(progress, 0.6921, 0.8432, 0.038);
  scene.line(BINARY.x1, ingressTop - 220, BINARY.x1, BINARY.y1, palette.accent, ingressAlpha * (1 - edgeOutline) * 0.45, 1.2, [8, 8]);
  scene.panel(INGRESS.x, INGRESS.y, ingressW, DOOR_H, ingressAlpha, 0.25, palette.accent);
  ctx.fillStyle = rgba(palette.accent, ingressAlpha * 0.6);
  ctx.fillRect(ingressRight - 18, INGRESS.y - 20, 4, 40);
  scene.text('INGRESS', ingressLeft + 22, INGRESS.y - 12, palette.ink, ingressAlpha * far);
  scene.tag('leased for the service', ingressLeft + 22, INGRESS.y + 13, ingressAlpha * 0.9 * scene.richText);
  const factAlpha = full * ingressFacts * scene.richText;
  const factStep = scene.px(18);
  const factBase = ingressTop - factStep;
  scene.tag('tcp only · no TLS termination · no HTTP parsing', ingressRight, factBase, factAlpha, palette.muted, 'right');
  scene.tag('service endpoint · host listener owned by nimbus', ingressRight, factBase - factStep, factAlpha, palette.accentText, 'right');

  // The forwarding lane: from the door into the right edge of the service
  // card, then from under the card down onto the box. The port it lands on
  // is a socket in the box top, drawn like the egress gate in its wall.
  const forwardIn = clamp(forward * 2);
  const forwardDrop = clamp(forward * 2 - 1);
  scene.line(ingressLeft, INGRESS.y, mix(ingressLeft, SERVICE.right, forwardIn), INGRESS.y, palette.accent, full * forwardIn * 0.7, 1.4);
  // Bytes keep coming in through the door.
  scene.lineTraffic(ingressLeft, INGRESS.y, mix(ingressLeft, SERVICE.right, forwardIn), INGRESS.y, palette.accent, full * forwardIn * 0.9, { count: 2, speed: 2.4, reverse: true });
  const serviceBottom = SERVICE.top + serviceCard(scene, progress).serviceH;
  scene.line(INGRESS.landX, serviceBottom, INGRESS.landX, mix(serviceBottom, top - 8, forwardDrop), palette.accent, full * forwardDrop * 0.7, 1.4);
  scene.lineTraffic(INGRESS.landX, serviceBottom, INGRESS.landX, mix(serviceBottom, top - 8, forwardDrop), palette.accent, full * forwardDrop * 0.9, { count: 1, speed: 2.4, phase: 0.5 });
  scene.roundRect(INGRESS.landX - 12, top - 8, 24, 16, 4);
  ctx.fillStyle = rgba(palette.background, full * forwardDrop);
  ctx.fill();
  ctx.lineWidth = scene.px(1.2);
  ctx.strokeStyle = rgba(palette.accent, full * forwardDrop);
  ctx.stroke();
  scene.tag('guest port', INGRESS.landX + scene.px(12), top - 200, full * forwardDrop * scene.richText, palette.muted);
}

// The service card's size. Its rows are laid out in screen pixels from the
// top, so the card grows to fit them; the ingress lane starts from under it.
// The service steps back to an outline once the sessions chapter builds on
// it, and returns for the pullback.
function servicePast(progress: number) {
  return smoothstep(range(progress, 0.5961, 0.6573)) * (1 - smoothstep(range(progress, 0.6972, 0.7078)));
}

// The outline keeps the label height, so the card only opens with its rows.
function serviceCard(scene: Scene, progress: number) {
  const rich = scene.rich * (1 - servicePast(progress));
  const serviceW = mix(SERVICE.w, Math.max(SERVICE.w, scene.px(340)), rich);
  const serviceRows = [24 + scene.px(24), scene.px(16), scene.px(22), scene.px(24), scene.px(16), scene.px(16)];
  const serviceFullH = serviceRows.reduce((sum, row) => sum + row, 0) + scene.px(18);
  const serviceH = mix(52, serviceFullH, rich);
  return { serviceW, serviceRows, serviceH };
}

// Services and sessions: the service `agent` is the name the box runs behind,
// declared in compose, and the session is a lease on exactly one target.
function drawServices(scene: Scene, progress: number) {
  const full = visibilityWindow(progress, 0.4291, 0.7983, 0.0333);
  const past = servicePast(progress);
  const alpha = full * (1 - past * 0.62);
  const serviceText = scene.richText * (1 - past);
  if (full <= 0.01 || !scene.inView(SESSIONS.xs[0] - SESSIONS.w, SERVICE_REST.x + 320, 120, SESSIONS.top - 160, SANDBOX.y)) return;
  const { palette } = scene;
  const { far, rich } = scene;
  // Chapter 11: the service, where it is declared, and the box it backs.
  const service = smoothstep(range(progress, 0.5523, 0.5625));
  const sources = smoothstep(range(progress, 0.5575, 0.5676));
  const backs = smoothstep(range(progress, 0.5608, 0.571));
  // Chapter 13: the sessions above and the leases they hold.
  const session = smoothstep(range(progress, 0.652, 0.664));
  const leases = smoothstep(range(progress, 0.664, 0.674));
  const sandboxTop = SANDBOX.y - SANDBOX.h / 2;

  // The service card, inside the process above the box.
  const { serviceW, serviceRows, serviceH } = serviceCard(scene, progress);
  const sx = SERVICE.right - serviceW;
  const sy = SERVICE.top;
  const serviceBottom = sy + serviceH;
  scene.panel(sx + serviceW / 2, sy + serviceH / 2, serviceW, serviceH, alpha * service, 0.3, palette.accent);
  scene.mono(12, 620);
  scene.text('SERVICE', sx + 20, sy + 24, palette.ink, alpha * service * far);
  const titleW = scene.measure('SERVICE');
  scene.chip('staticCatalog · generation 3', sx + 20 + titleW + scene.px(12), sy + 24, alpha * service * serviceText, 0.4, palette.accent);
  scene.mono(10.5);
  let rowY = sy + serviceRows[0];
  scene.text('agent · a tenant-scoped name', sx + 20, rowY, palette.muted, alpha * service * serviceText);
  rowY += serviceRows[1];
  scene.text('survives restarts · redeploys · backend changes', sx + 20, rowY, palette.muted, alpha * service * serviceText);
  // Where the name comes from: the compose file this one came from, or an
  // API call. Same resource, same verbs, so both sit on the one card.
  rowY += serviceRows[2];
  const sourceAlpha = alpha * sources * serviceText;
  const composeW = scene.chip('compose.yaml', sx + 20, rowY, sourceAlpha, 1, palette.accent);
  scene.chip('services.create', sx + 20 + composeW + scene.px(8), rowY, sourceAlpha, 0.2, palette.accent);
  rowY += serviceRows[3];
  scene.tag('same resource · same verbs', sx + 20, rowY, sourceAlpha, palette.accentText);
  rowY += serviceRows[4];
  scene.mono(10.5);
  scene.text('backend: sandbox · built-in · external endpoint', sx + 20, rowY, palette.muted, alpha * service * serviceText);
  rowY += serviceRows[5];
  scene.text('waitUntil: "ready" · fenced by generation', sx + 20, rowY, palette.muted, alpha * service * serviceText);

  // The service backs the sandbox: a lane down through the process, past the
  // tenant door, across the edge into the box on the host. The notes stand
  // left of the lane, clear of the request that rides it.
  const backsAlpha = alpha * backs;
  scene.line(SERVICE.lane, serviceBottom, SERVICE.lane, mix(serviceBottom, sandboxTop, backs), palette.accent, backsAlpha * 0.7, 1.6);
  // Calls to the name keep reaching the box.
  scene.lineTraffic(SERVICE.lane, serviceBottom, SERVICE.lane, mix(serviceBottom, sandboxTop, backs), palette.accent, backsAlpha * 0.9, { count: 2, speed: 2.4 });
  const noteX = SERVICE.lane - scene.px(40);
  const noteY = serviceBottom + scene.px(24);
  scene.tag('backs the sandbox', noteX, noteY, backsAlpha * serviceText, palette.accentText, 'right');
  scene.tag('owner: service agent', noteX, noteY + scene.px(18), backsAlpha * serviceText, palette.muted, 'right');
  scene.tag('like a Kubernetes Service', noteX, noteY + scene.px(36), backsAlpha * serviceText, palette.accentText, 'right');
  // The service is a record the control plane keeps: a dashed lane from the
  // card's underside, between the notes and the lane it backs the box with,
  // down to the control plane card that holds the record.
  const recordX = CONTROL.x + CONTROL.w / 2 - 30;
  scene.line(recordX, serviceBottom, recordX, mix(serviceBottom, CONTROL.top, sources), palette.accent, alpha * sources * 0.4, 1.2, [5, 7]);

  // Chapter 13: the sessions above the service. A target takes as many
  // sessions as the work needs, and each one is its own lease. Two lease
  // the service by name; the third leases the same box by its id and drops
  // past the service straight into it. The cards hold screen-sized labels,
  // so their rows are spaced in screen pixels and the detail card is a
  // little wider than the label card.
  const cardTop = SESSIONS.top;
  const sessionRows = [24 + scene.px(22), scene.px(20)];
  const sessionFullH = sessionRows.reduce((sum, row) => sum + row, 0) + scene.px(18);
  const sessionH = mix(52, sessionFullH, rich);
  const sessionW = mix(200, Math.max(SESSIONS.w, scene.px(150)), rich);
  const sessionStep = Math.max(SESSIONS.xs[1] - SESSIONS.xs[0], sessionW + scene.px(10));
  const sessionXs = SESSIONS.xs.map((_, index) => SESSIONS.xs[0] + index * sessionStep);
  const cardBottom = cardTop + sessionH;
  const cards = [
    { x: sessionXs[0], target: 'service · agent', channels: 'stdio' },
    { x: sessionXs[1], target: 'service · agent', channels: 'stdio · files' },
    { x: sessionXs[2], target: 'sandbox · by id', channels: 'files' },
  ];
  const rowLeft = sessionXs[0] - sessionW / 2;
  scene.mono(12, 620);
  const sessionTitleW = scene.measure('SESSION');
  // At a phone's zoom the title outgrows its card and would run into the
  // next one, so the row carries one title above the cards instead.
  const titled = sessionTitleW + 32 <= sessionW;
  scene.text('SESSIONS', rowLeft, cardTop - scene.px(16), palette.ink, full * session * far * (titled ? 0 : 1));
  cards.forEach((card, index) => {
    const reveal = clamp(session * 1.6 - index * 0.3);
    const cx = card.x - sessionW / 2;
    scene.panel(card.x, cardTop + sessionH / 2, sessionW, sessionH, full * reveal, 0.3, palette.accent);
    scene.mono(12, 620);
    scene.text('SESSION', cx + 16, cardTop + 24, palette.ink, full * reveal * far * (titled ? 1 : 0));
    scene.chip('open', cx + 16 + sessionTitleW + scene.px(10), cardTop + 24, full * reveal * scene.richText, 1, palette.accent);
    scene.mono(10.5);
    let sessionRowY = cardTop + sessionRows[0];
    scene.text(card.target, cx + 16, sessionRowY, palette.muted, full * reveal * scene.richText);
    sessionRowY += sessionRows[1];
    scene.chip(card.channels, cx + 16, sessionRowY, full * reveal * scene.richText, 0.6, palette.accent, 'left', sessionW - 32);
  });
  scene.tag('one lease each · ttl 15 min · 1 h max', rowLeft, cardTop - scene.px(18), full * session * scene.richText, palette.accentText);

  // The leases. By name: straight down onto the service.
  const leaseAlpha = full * leases;
  [0, 1].forEach((index) => {
    const x = sessionXs[index];
    scene.line(x, cardBottom, x, mix(cardBottom, sy, leases), palette.accent, leaseAlpha * 0.7, 1.6);
    // A session carries stdio and files both ways while its lease holds.
    scene.lineTraffic(x, cardBottom, x, mix(cardBottom, sy, leases), palette.accent, leaseAlpha * 0.9, { count: 1, speed: 2.2, phase: index * 0.45 });
    scene.lineTraffic(x, cardBottom, x, mix(cardBottom, sy, leases), palette.accent, leaseAlpha * 0.9, { count: 1, speed: 2.2, phase: 0.5 + index * 0.45, reverse: true });
  });
  scene.tag('lease · by name', sessionXs[1] + scene.px(12), (cardBottom + sy) / 2, leaseAlpha * scene.richText, palette.accentText);
  // By id: past the service, between it and the proxy, straight into the box.
  const idX = sessionXs[2];
  scene.line(idX, cardBottom, idX, mix(cardBottom, sandboxTop, leases), palette.accent, leaseAlpha * 0.5, 1.2, [6, 8]);
  scene.lineTraffic(idX, cardBottom, idX, mix(cardBottom, sandboxTop, leases), palette.accent, leaseAlpha * 0.9, { count: 1, speed: 1.6, phase: 0.3 });
  scene.tag('lease · by id', idX + scene.px(40), sy + 30, leaseAlpha * scene.richText, palette.muted);
}

// Compose: the compose file, the services it names, and the sandbox each
// one runs in, all on this host beside the engine. The board sits right of
// the host, in its own frame, and only in its own window: it lies under the
// egress copy column and beside the laptop otherwise. Rows are laid out in
// world units for the composed zoom; the text rows inside them are screen
// spaced, so the tiles are tall enough for three rows at that zoom.
const WORKLOAD_TILE = { w: 260, xs: [7460, 7760, 8060] };
function drawWorkloads(scene: Scene, progress: number) {
  const alpha = visibilityWindow(progress, 0.6067, 0.6573, 0.019);
  if (alpha <= 0.01) return;
  const left = WORKLOADS.x - WORKLOADS.w / 2;
  const top = WORKLOADS.y - WORKLOADS.h / 2;
  if (!scene.inView(left, left + WORKLOADS.w, 120, top, top + WORKLOADS.h + 200)) return;
  const { ctx, palette } = scene;
  const { far, rich, richText } = scene;
  // The compose card first, then the lanes fan out to the services, then the
  // sandboxes light under them, then the declared backends row.
  const compose = smoothstep(range(progress, 0.6067, 0.6118));
  const fan = smoothstep(range(progress, 0.6109, 0.6184));
  const boxes = smoothstep(range(progress, 0.6175, 0.6251));
  const declared = smoothstep(range(progress, 0.6232, 0.6308));
  const rowGap = scene.px(18);
  // Text rows are spaced in screen px, so every tile height is derived from
  // the zoom and the rows stack cumulatively. The board grows with them.
  const composeW = mix(240, 520, rich);
  const composeH = mix(48, 22 + rowGap + scene.px(18), rich);
  const composeTop = top + 60;
  const composeBottom = composeTop + composeH;
  const tileH = mix(44, 22 + rowGap * 2 + scene.px(16), rich);
  const serviceTop = composeBottom + 60;
  const boxTop = serviceTop + tileH + 50;
  const rowTop = boxTop + tileH + 40;
  const rowH = mix(44, 22 + rowGap * 2 + scene.px(10), rich);
  const boardH = rowTop + rowH - top + mix(40, scene.px(58), rich);

  scene.panel(WORKLOADS.x, top + boardH / 2, WORKLOADS.w, boardH, alpha, 0);
  const titleDrop = Math.max(22, scene.px(15));
  scene.mono(12, 620);
  scene.text('COMPOSE', left + 24, top + titleDrop, palette.ink, alpha * far);
  const titleW = scene.measure('COMPOSE');
  scene.tag('one host · services beside the engine', left + 24 + titleW + scene.px(12), top + titleDrop, alpha * richText, palette.muted);

  // The compose file, left of the request point.
  const composeX = WORKLOADS.x - 20;
  const composeY = composeTop + composeH / 2;
  scene.panel(composeX, composeY, composeW, composeH, alpha * compose, 0.3, palette.accent);
  scene.mono(11, 620);
  scene.text('COMPOSE.YAML', composeX - composeW / 2 + 16, composeTop + 22, palette.ink, alpha * compose * far);
  scene.mono(10.5);
  scene.text('services: web · agent · worker', composeX - composeW / 2 + 16, composeTop + 22 + rowGap, palette.muted, alpha * compose * richText);
  scene.chip('nimbus compose up', composeX + composeW / 2 - 16, composeTop + 22, alpha * compose * richText, 1, palette.accent, 'right');

  // Three services, each in its own sandbox.
  const services = [
    { name: 'WEB', sub: 'port 8080 · ingress', box: 'MICROVM', runtime: 'libkrun · default' },
    { name: 'AGENT', sub: 'python agent.py', box: 'MICROVM', runtime: 'libkrun · default' },
    { name: 'WORKER', sub: 'x-nimbus: crun', box: 'CONTAINER', runtime: 'crun' },
  ];
  services.forEach((service, index) => {
    const x = WORKLOAD_TILE.xs[index];
    const lit = clamp(fan * 1.8 - index * 0.3);
    // The lane from the compose card down into the service.
    scene.laneVertical(WORKLOADS.x + (index - 1) * 40, composeBottom, x, mix(composeBottom, serviceTop, lit), palette.accent, alpha * lit * 0.6, 1.4);
    scene.laneVerticalTraffic(WORKLOADS.x + (index - 1) * 40, composeBottom, x, mix(composeBottom, serviceTop, lit), palette.accent, alpha * lit * 0.9, { count: 1, speed: 2, phase: index * 0.33 });
    scene.panel(x, serviceTop + tileH / 2, WORKLOAD_TILE.w, tileH, alpha * lit, 0.3, palette.accent);
    scene.mono(11, 620);
    scene.text(service.name, x - WORKLOAD_TILE.w / 2 + 16, serviceTop + 22, palette.ink, alpha * lit * far);
    scene.mono(10.5);
    scene.text(service.sub, x - WORKLOAD_TILE.w / 2 + 16, serviceTop + 22 + rowGap, palette.muted, alpha * lit * richText);
    scene.tag('backend: sandbox', x - WORKLOAD_TILE.w / 2 + 16, serviceTop + 22 + rowGap * 2, alpha * lit * richText, palette.accentText);
    // One sandbox under each service.
    const boxLit = clamp(boxes * 1.8 - index * 0.3);
    scene.line(x, serviceTop + tileH, x, mix(serviceTop + tileH, boxTop, boxLit), palette.accent, alpha * boxLit * 0.6, 1.4);
    scene.lineTraffic(x, serviceTop + tileH, x, mix(serviceTop + tileH, boxTop, boxLit), palette.accent, alpha * boxLit * 0.9, { count: 1, speed: 2.2, phase: 0.5 + index * 0.33 });
    scene.panel(x, boxTop + tileH / 2, WORKLOAD_TILE.w, tileH, alpha * boxLit, 0.15);
    scene.mono(11, 620);
    scene.text(service.box, x - WORKLOAD_TILE.w / 2 + 16, boxTop + 22, palette.ink, alpha * boxLit * far);
    scene.mono(10.5);
    scene.text(service.runtime, x - WORKLOAD_TILE.w / 2 + 16, boxTop + 22 + rowGap, palette.muted, alpha * boxLit * richText);
    scene.tag('one sandbox', x - WORKLOAD_TILE.w / 2 + 16, boxTop + 22 + rowGap * 2, alpha * boxLit * richText, palette.muted);
  });

  // The other backend kinds, declared today, drawn dashed. The row sits under
  // the sandboxes with a note on what runs today and what is planned.
  const declaredTiles = [
    { x: 7595, w: 610, name: 'BUILT-IN', lines: ['loadBalancer · serviceDiscovery', 'browser · modelGateway'], chip: 'declared · planned' },
    { x: 8080, w: 300, name: 'EXTERNAL', lines: ['endpoint', 'health check'], chip: 'declared' },
  ];
  declaredTiles.forEach((tile, index) => {
    const lit = clamp(declared * 1.6 - index * 0.4);
    const x0 = tile.x - tile.w / 2;
    scene.roundRect(x0, rowTop, tile.w, rowH, 10);
    ctx.lineWidth = scene.px(1);
    ctx.strokeStyle = rgba(palette.ink, alpha * lit * 0.3);
    ctx.setLineDash([scene.px(6), scene.px(6)]);
    ctx.stroke();
    ctx.setLineDash([]);
    scene.mono(11, 620);
    scene.text(tile.name, x0 + 16, rowTop + 22, palette.ink, alpha * lit * far);
    const nameW = scene.measure(tile.name);
    scene.chip(tile.chip, x0 + 16 + nameW + scene.px(10), rowTop + 22, alpha * lit * richText, 0.2, palette.accent, 'left', tile.w - nameW - 40);
    scene.mono(10.5);
    tile.lines.forEach((line, lineIndex) => {
      scene.text(line, x0 + 16, rowTop + 22 + rowGap * (lineIndex + 1), palette.muted, alpha * lit * richText);
    });
  });
  scene.tag('one service = one sandbox today · replicas and a load balancer: planned', left + 24, rowTop + rowH + scene.px(30), alpha * declared * richText, palette.accentText);
}

function drawBinary(scene: Scene, progress: number) {
  const outline = visibilityWindow(progress, 0.6921, 0.8432, 0.038);
  if (outline <= 0.01) return;
  const { ctx, palette } = scene;
  const draw = smoothstep(range(progress, 0.7006, 0.7388));

  // Match cut: the whole outline collapses into a slot on the cloud rack while
  // the camera travels there, between the tenants copy and the cloud copy.
  // The collapse runs with the camera travel, so the shrinking outline stays
  // in the frame all the way into the slot.
  const collapse = smoothstep(range(progress, 0.7983, 0.8089));
  const slotH = RACK.h / RACK.slots;
  const slot = {
    x0: RACK.x - RACK.w / 2 + 16,
    x1: RACK.x + RACK.w / 2 - 16,
    y0: RACK.y - RACK.h / 2 + RACK_SLOT * slotH + 8,
    y1: RACK.y - RACK.h / 2 + (RACK_SLOT + 1) * slotH - 8,
  };
  // The top edge lifts to hold the tenant deck while it is fanned out.
  const deck = tenantDeck(scene, progress);
  // The centre travels with the camera; the size shrinks geometrically, so
  // the outline reads smaller in every frame while the camera zooms in.
  const topY = BINARY.y0 - deck.growth;
  const w = (BINARY.x1 - BINARY.x0) * Math.pow((slot.x1 - slot.x0) / (BINARY.x1 - BINARY.x0), collapse);
  const h = (BINARY.y1 - topY) * Math.pow((slot.y1 - slot.y0) / (BINARY.y1 - topY), collapse);
  const cx = mix((BINARY.x0 + BINARY.x1) / 2, (slot.x0 + slot.x1) / 2, collapse);
  const cy = mix((topY + BINARY.y1) / 2, (slot.y0 + slot.y1) / 2, collapse);
  const x0 = cx - w / 2;
  const x1 = cx + w / 2;
  const y0 = cy - h / 2;
  const y1 = cy + h / 2;
  if (!scene.inView(x0, x1, 400, y0, y1)) return;
  const perimeter = 2 * (x1 - x0 + y1 - y0);
  scene.roundRect(x0, y0, x1 - x0, y1 - y0, 24 * (1 - collapse) + 6);
  ctx.lineWidth = scene.px(1.5 + collapse);
  ctx.strokeStyle = rgba(palette.accent, outline * (0.7 + collapse * 0.3));
  ctx.setLineDash([perimeter * draw, perimeter]);
  ctx.lineDashOffset = -perimeter * 0.02;
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.fillStyle = rgba(palette.accent, outline * 0.03 * draw + collapse * collapse * 0.2);
  ctx.fill();
  drawBinaryLabels(scene, progress, x0, x1, y0, y1, outline);
}

const modules: [string, string[]][] = [
  ['NETWORK', ['client protocols', 'one address']],
  ['COMPUTE', ['V8 · Node targets', 'same process']],
  ['STORAGE', ['SQLite default · zero setup', 'Postgres · MySQL', 'libSQL · a connection', 'objects · volumes · KV']],
  ['AGENTS', ['sandboxes · egress']],
  ['WORKLOADS', ['services · sessions']],
  ['AUTH', ['principals · tenants', 'admin token']],
];

// Labels leave with the chapter copy, before the camera drops to the tenants,
// so no text sits under the tenant heading. They sit outside the outline: the
// name above, the module row below, so the frame around the system stays
// clean. The camera is far back here and fonts sit at their minimum screen
// size, so spacing is set in screen pixels and the module row is measured:
// six described columns where they fit, six titles where only those fit,
// and one line on a phone.
function drawBinaryLabels(scene: Scene, progress: number, x0: number, x1: number, y0: number, y1: number, outline: number) {
  const labelAlpha = outline * smoothstep(range(progress, 0.7057, 0.7329)) * (1 - smoothstep(range(progress, 0.7439, 0.7536)));
  if (labelAlpha <= 0.01) return;
  const { ctx, palette } = scene;
  const inset = scene.px(4);
  const available = x1 - x0 - inset * 2;
  const gap = scene.px(12);

  scene.mono(10.5);
  const lineW = Math.max(...modules.flatMap(([, lines]) => lines).map((line) => scene.measure(line)));
  scene.mono(11, 620);
  const titleW = Math.max(...modules.map(([title]) => scene.measure(title)));
  const spacingFor = (columnW: number) => (available - columnW) / (modules.length - 1);
  const mode = spacingFor(lineW) >= lineW + gap / 2 ? 'columns' : spacingFor(titleW) >= titleW + gap ? 'titles' : 'line';

  scene.sans(scene.px(30), 380);
  scene.text('nimbus', x0 + inset - scene.px(2), y0 - scene.px(50), palette.ink, labelAlpha);
  scene.mono(11, 600);
  scene.text('ONE PROCESS · ONE RUST BINARY · ONE TRUST MODEL', x0 + inset, y0 - scene.px(20), palette.muted, labelAlpha);

  const reveal = smoothstep(range(progress, 0.7078, 0.7268));
  // The hosted engines and the object store sit below the outline, so the
  // module row hangs beneath them.
  const below = y1 + EXTERNAL_DEPTH;
  const my = below + scene.px(30);
  if (mode === 'line') {
    scene.mono(11, 620);
    const titles = modules.map(([title]) => title);
    const line = titles.join(' · ');
    if (scene.measure(line) <= available) {
      scene.text(line, (x0 + x1) / 2, my, palette.ink, labelAlpha * reveal, 'center');
    } else {
      const split = Math.ceil(titles.length / 2);
      scene.text(titles.slice(0, split).join(' · '), (x0 + x1) / 2, my, palette.ink, labelAlpha * reveal, 'center');
      scene.text(titles.slice(split).join(' · '), (x0 + x1) / 2, my + scene.px(16), palette.ink, labelAlpha * reveal, 'center');
    }
    return;
  }
  const spacing = spacingFor(mode === 'columns' ? lineW : titleW);
  const lineH = scene.px(16);
  modules.forEach(([title, lines], index) => {
    const lit = clamp(reveal * 1.8 - index * 0.16);
    const mx = x0 + inset + spacing * index;
    ctx.beginPath();
    ctx.moveTo(mx, below);
    ctx.lineTo(mx, below + (my - below - scene.px(12)) * lit);
    ctx.lineWidth = scene.px(1);
    ctx.strokeStyle = rgba(palette.accent, labelAlpha * lit * 0.6);
    ctx.stroke();
    scene.mono(11, 620);
    scene.text(title, mx, my, palette.ink, labelAlpha * lit);
    if (mode !== 'columns') return;
    scene.mono(10.5);
    lines.forEach((line, lineIndex) => {
      scene.text(line, mx, my + lineH * (lineIndex + 1), palette.muted, labelAlpha * lit);
    });
  });
}

// Tenants, at the scale of the process. The interior past the fork is one
// tenant's slice, drawn on the front sheet of a deck; two more sheets sit
// behind it: the same machinery, its own walls. Each sheet behind steps up by
// one strip and shows its name on that strip, like a stack of folders; the
// front sheet carries its name at its own top left corner. The process
// outline lifts by the two strips so the deck stays inside it. The engine's
// tenants cell is the door, so admission happens once.
function tenantDeck(scene: Scene, progress: number) {
  // The deck leaves with the interior, as the outline collapses into the rack.
  const alpha = visibilityWindow(progress, 0.7533, 0.8017, 0.019);
  const reveal = smoothstep(range(progress, 0.7584, 0.7741));
  // Each sheet carries its name on a title band set in screen pixels, so the
  // bands read at every zoom; the sideways step is capped in world units so
  // the back sheet stays clear of the engine and its door tag.
  const compact = scene.viewport === 'compact';
  const stripH = scene.px(compact ? 28 : 46);
  const dx = Math.min(scene.px(14), 80) * reveal;
  const dy = stripH * reveal;
  // The front sheet's own band sits above the runtime; where the band is
  // taller than that room, the front sheet's top edge lifts too.
  const lift = Math.max(0, stripH + scene.px(6) - (RUNTIME.y - RUNTIME.h / 2 - SHEET.y0)) * reveal;
  const growth = stripH * (SHEETS.length - 1) * reveal + lift + scene.px(compact ? 8 : 12) * reveal;
  // While the deck is fanned out, the machinery on the front sheet steps back
  // so the sheets, their names and the walls are what the frame is about.
  const dim = 1 - 0.38 * alpha * reveal;
  return { alpha, reveal, dx, dy, lift, stripH, growth, dim, nameSize: compact ? 12 : 15 };
}

function drawTenantSheets(scene: Scene, progress: number) {
  const deck = tenantDeck(scene, progress);
  if (deck.alpha <= 0.01 || !scene.inView(SHEET.x0 - 300, SHEET.x1, 200, SHEET.y0 - deck.growth - 200, SHEET.y1 + 400)) return;
  const { ctx, palette } = scene;
  const w = SHEET.x1 - SHEET.x0;
  const h = SHEET.y1 - SHEET.y0 + deck.lift;
  const radius = 18;
  // Back to front. The front sheet is the ground the drawn world stands on;
  // each sheet behind is tinted deeper, and every sheet has a title band.
  for (let index = SHEETS.length - 1; index >= 0; index -= 1) {
    const x = SHEET.x0 - deck.dx * index;
    const y = SHEET.y0 - deck.lift - deck.dy * index;
    scene.roundRect(x, y, w, h, radius);
    ctx.fillStyle = rgba(mixRgb(palette.background, palette.accent, 0.1 + index * 0.12), deck.alpha);
    ctx.fill();
    // The band: the top of the sheet, clipped to the sheet's rounded corners.
    ctx.save();
    scene.roundRect(x, y, w, h, radius);
    ctx.clip();
    ctx.fillStyle = rgba(palette.accent, deck.alpha * deck.reveal * (index === 0 ? 0.2 : 0.16));
    ctx.fillRect(x, y, w, deck.stripH);
    ctx.restore();
    scene.line(x, y + deck.stripH, x + w, y + deck.stripH, palette.accent, deck.alpha * deck.reveal * 0.55, 1);
    scene.roundRect(x, y, w, h, radius);
    ctx.lineWidth = scene.px(1.5);
    ctx.strokeStyle = rgba(palette.accent, deck.alpha * (index === 0 ? 0.95 : 0.8));
    ctx.stroke();
  }
}

type Callout = { lines: string[]; x: number; y: number; align: 'left' | 'right' | 'center'; pin: [number, number] };

function drawTenants(scene: Scene, progress: number) {
  const deck = tenantDeck(scene, progress);
  if (deck.alpha <= 0.01 || !scene.inView(SHEET.x0 - 300, BINARY.x1, 200, SHEET.y0 - deck.growth - 200, BINARY.y1 + 800)) return;
  const { ctx, palette } = scene;
  const { alpha, dx, dy, lift, stripH, nameSize } = deck;
  scene.textAlpha = 1;
  const labels = smoothstep(range(progress, 0.7648, 0.78));
  const admit = smoothstep(range(progress, 0.7618, 0.7724));
  const callouts = smoothstep(range(progress, 0.7709, 0.7864));
  const crosses = smoothstep(range(progress, 0.7783, 0.7864));
  const facts = smoothstep(range(progress, 0.7797, 0.7881));
  // Callouts and the door tag need the room a desktop frame has at the
  // process scale; a tablet or a phone shows the deck and its labels only.
  const room = smoothstep(range(scene.zoom, 0.092, 0.1));

  // Names on the bands: the front sheet's above the runtime; each sheet
  // behind on the band it shows above the sheet in front of it. On a desktop
  // the back sheets also say what each tenant owns.
  const w = SHEET.x1 - SHEET.x0;
  SHEETS.forEach((sheet, index) => {
    const show = clamp(labels * 1.6 - index * 0.3);
    const x = SHEET.x0 - dx * index;
    const nameY = SHEET.y0 - lift - dy * index + stripH / 2;
    const lit = index === 0 ? 1 : 0;
    // Every card says what it is: a tenant chip, then the tenant's id.
    const chipW = scene.chip('tenant', x + scene.px(12), nameY, alpha * show, lit, palette.accent, 'left', scene.px(120));
    const nameX = x + scene.px(12) + chipW + scene.px(10);
    scene.mono(nameSize, 700);
    scene.text(sheet.id, nameX, nameY, mixRgb(palette.ink, palette.accentText, lit), alpha * show * (index === 2 ? 0.85 : 1));
    const idW = scene.measure(sheet.id);
    if (sheet.note) {
      // A tiny still has no room for the note beside the id; the id stands.
      scene.mono(nameSize - 2);
      const noteX = nameX + idW + scene.px(12);
      if (noteX + scene.measure(sheet.note) < x + w - scene.px(10)) {
        scene.text(sheet.note, noteX, nameY, palette.muted, alpha * show * 0.9);
      }
    }
    if (sheet.owns && scene.viewport !== 'compact') {
      scene.mono(11);
      const ownsW = scene.measure(sheet.owns);
      if (ownsW + idW + chipW + scene.px(150) < w) {
        scene.text(sheet.owns, x + w - scene.px(16), nameY, palette.muted, alpha * show * 0.9, 'right');
      }
    }
  });

  // What stays true hangs under the outline, below the hosted engines and
  // the object store.
  const rowY0 = BINARY.y1 + EXTERNAL_DEPTH + scene.px(26);
  const rowStep = scene.px(18);

  // No read crosses a wall: a cross on each exposed edge between sheets.
  const size = Math.min(scene.px(3), dx * 0.28);
  [1, 2].forEach((index) => {
    const x = SHEET.x0 - dx * index + dx / 2;
    const y = 520;
    scene.line(x - size, y - size, x + size, y + size, palette.accent, alpha * crosses, 1.4);
    scene.line(x - size, y + size, x + size, y - size, palette.accent, alpha * crosses, 1.4);
  });

  // What stays true, right-aligned under the outline beside the labels.
  if (scene.viewport !== 'compact') {
    scene.tag('cross-tenant reads · not expressible', BINARY.x1 - scene.px(4), rowY0, alpha * facts, palette.accentText, 'right');
    scene.tag('per-tenant budgets · 429 when exceeded', BINARY.x1 - scene.px(4), rowY0 + rowStep, alpha * facts, palette.muted, 'right');
    scene.tag('nimbus start · production isolation always on', BINARY.x1 - scene.px(4), rowY0 + rowStep * 2, alpha * facts, palette.muted, 'right');
  }

  if (room <= 0.01) {
    ctx.setLineDash([]);
    return;
  }
  // The door: one engine admits the request once; the tenants cell lights.
  scene.tag('admitted once', ENGINE.x + ENGINE.w / 2 - 10, ENGINE.y + ENGINE.h / 2 + scene.px(14), alpha * admit * room, palette.accentText, 'right');

  // Four walls, pinned to the parts of the front sheet they belong to.
  const runtimeRight = RUNTIME.x + RUNTIME.w / 2;
  const hostRight = HOST.x + HOST.w / 2;
  const list: Callout[] = [
    { lines: ['own runtime', 'budget'], x: runtimeRight + scene.px(18), y: 250, align: 'left', pin: [runtimeRight, 110] },
    { lines: ['own SQLite file'], x: SHELF.left - scene.px(2), y: 1090, align: 'left', pin: [embeddedEngines[0].x - ENGINE_TILE.w / 2 + 16, ENGINE_TILE.embeddedY + ENGINE_TILE.h / 2 - 12] },
    { lines: ['own blob store + key'], x: OBJECT_STORE.x, y: OBJECT_STORE.y + OBJECT_STORE.h / 2 + scene.px(26), align: 'center', pin: [OBJECT_STORE.x, OBJECT_STORE.y + OBJECT_STORE.h / 2] },
    { lines: ['own sandboxes', 'own volumes'], x: hostRight + scene.px(14), y: SANDBOX.y - scene.px(8), align: 'left', pin: [hostRight, SANDBOX.y] },
  ];
  const lineH = scene.px(16);
  list.forEach((callout, index) => {
    const show = clamp(callouts * 1.9 - index * 0.3) * room;
    if (show <= 0.01) return;
    const anchorX = callout.align === 'left' ? callout.x - scene.px(7) : callout.align === 'right' ? callout.x + scene.px(7) : callout.x;
    const anchorY = callout.align === 'center' ? callout.y - scene.px(9) * Math.sign(callout.y - callout.pin[1]) : callout.y;
    scene.line(callout.pin[0], callout.pin[1], mix(callout.pin[0], anchorX, show), mix(callout.pin[1], anchorY, show), palette.accent, alpha * show * 0.6, 1);
    ctx.beginPath();
    ctx.arc(callout.pin[0], callout.pin[1], scene.px(2.5), 0, Math.PI * 2);
    ctx.fillStyle = rgba(palette.accent, alpha * show);
    ctx.fill();
    scene.mono(11, 600);
    callout.lines.forEach((line, row) => {
      scene.text(line, callout.x, callout.y + lineH * row, palette.ink, alpha * show, callout.align);
    });
  });
  ctx.setLineDash([]);
}

// Your cloud: a region outline around one rack, up above the process; the
// binary lands in a slot. In the cluster chapter the rack duplicates: two
// more racks slide out of it, the region widens to hold them, and the mesh
// between the three is drawn beneath. Everything past the first rack is
// roadmap and the board says so.
function clusterSpread(progress: number) {
  return smoothstep(range(progress, 0.862, 0.874));
}

// The three racks, left to right. Their keys are illustrative: a node's
// identity in the planned mesh is an Ed25519 public key, not an address.
const RACKS = [
  { name: 'node A', note: 'this host', key: 'Ed25519 · 7f3a…c21e', role: 'owner: demo', offset: 0 },
  { name: 'node B', note: 'planned', key: 'Ed25519 · 9b12…e07d', role: 'learner → voter', offset: -1 },
  { name: 'node C', note: 'planned', key: 'Ed25519 · 4e88…a3f1', role: 'relay · opt-in', offset: 1 },
];
// This host is the middle rack; the copies slide out to either side.
const HOME_RACK = 0;

function rackX(index: number, spread: number) {
  return RACK.x + RACKS[index].offset * RACK.spread * spread;
}

function drawRack(scene: Scene, x: number, alpha: number, settled: number, label: string) {
  const { ctx, palette } = scene;
  const rx = x - RACK.w / 2;
  const ry = RACK.y - RACK.h / 2;
  const slotH = RACK.h / RACK.slots;
  scene.panel(x, RACK.y, RACK.w, RACK.h, alpha, 0.15);
  for (let index = 0; index < RACK.slots; index += 1) {
    const sy = ry + index * slotH;
    scene.roundRect(rx + 16, sy + 8, RACK.w - 32, slotH - 16, 6);
    ctx.lineWidth = scene.px(1);
    ctx.strokeStyle = rgba(palette.ink, alpha * 0.16);
    ctx.stroke();
    // Activity lights: each slot is busy on its own beat.
    for (let dot = 0; dot < 3; dot += 1) {
      const led = scene.ambient ? 0.55 + 0.45 * scene.breath(0.9, index * 1.3 + dot * 2.1) : 1;
      ctx.fillStyle = rgba(palette.ink, alpha * 0.4 * led);
      ctx.beginPath();
      ctx.arc(rx + RACK.w - 40 - dot * 12, sy + slotH / 2, 2, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  scene.mono(11.5, 600);
  const slotY = rackSlotY(RACK_SLOT);
  const nameW = scene.measure('nimbus');
  scene.mono(11);
  const commandW = scene.measure(label);
  // The request sits at the left end of the slot; the label starts right of it.
  const labelX = rx + 34 + scene.px(34);
  if (nameW + scene.px(46) + commandW <= RACK.w - 32 - 48) {
    scene.mono(11.5, 600);
    scene.text('nimbus', labelX, slotY, palette.accentText, alpha * settled);
    scene.tag(label, labelX + nameW + scene.px(12), slotY, alpha * settled, palette.ink);
  } else {
    scene.tag('nimbus', labelX, slotY, alpha * settled, palette.accentText);
  }
}

function drawCloud(scene: Scene, progress: number, time: number, ambient: boolean) {
  const alpha = visibilityWindow(progress, 0.7932, 1.14, 0.0095);
  const spread = clusterSpread(progress);
  const regionW = CLOUD.w + CLOUD.spreadW * spread;
  const regionH = CLOUD.h + CLOUD.spreadH * spread;
  const cx = CLOUD.x - regionW / 2;
  const cy = CLOUD.y - CLOUD.h / 2;
  if (alpha <= 0.01 || !scene.inView(cx, cx + regionW, 200, cy, cy + regionH + 400)) return;
  const { ctx, palette } = scene;
  const { far, richText } = scene;
  const rx = RACK.x - RACK.w / 2;
  const ry = RACK.y - RACK.h / 2;
  const settled = smoothstep(range(progress, 0.8136, 0.828));
  const region = smoothstep(range(progress, 0.7983, 0.8089));
  // Chapter 17: the racks duplicate, the mesh joins them, the roles land,
  // then packets ride the mesh. All of it is planned.
  const cluster = visibilityWindow(progress, 0.8595, 1.14, 0.0095);
  const mesh = smoothstep(range(progress, 0.872, 0.88));
  const roles = smoothstep(range(progress, 0.879, 0.886));
  // Packets ride as soon as the mesh has joined, so the chapter's rest
  // already shows the traffic.
  const packets = smoothstep(range(progress, 0.877, 0.882));

  // Region outline draws itself around the rack, then widens with the copies.
  const perimeter = 2 * (regionW + regionH);
  scene.roundRect(cx, cy, regionW, regionH, 28);
  ctx.lineWidth = scene.px(1);
  ctx.strokeStyle = rgba(palette.ink, alpha * 0.28);
  ctx.setLineDash([perimeter * region, perimeter]);
  ctx.stroke();
  ctx.setLineDash([]);
  // The header text has a screen-pixel floor. When the header strip above the
  // rack is thinner than that floor (far zoom, static stills), the header fades
  // so it never sits on the rack.
  scene.mono(11, 620);
  const labelW = scene.measure('DEPLOYMENT');
  const headRoom = (ry - cy) * scene.zoom;
  const sideRoom = (rx - RACK.spread * spread - cx - 28 - labelW) * scene.zoom;
  // With a tall enough strip the header stands above the racks and needs no
  // side room, so the copies can slide under it.
  const header = smoothstep(range(headRoom, 28, 40)) * Math.max(smoothstep(range(sideRoom, 0, 12)), smoothstep(range(headRoom, 40, 48)));
  scene.text('DEPLOYMENT', cx + 28, cy + scene.px(26), palette.ink, alpha * region * header);
  scene.mono(10.5, 600);
  const ingressW = scene.measure(':8080 · nimbus start') + scene.px(24);
  scene.mono(11);
  const hostTagW = scene.measure('any Linux host · VPS · bare metal');
  // At the cluster zoom the header strip is too short for the host tag
  // between the title and the ingress chip, so the tag yields.
  const hostTagRoom = (regionW - 56 - labelW - scene.px(12) - hostTagW - scene.px(16) - ingressW) * scene.zoom;
  const hostTag = smoothstep(range(hostTagRoom, 0, 12));
  if (scene.viewport === 'wide') {
    scene.tag('any Linux host · VPS · bare metal', cx + 28 + labelW + scene.px(12), cy + scene.px(26), alpha * region * header * hostTag * 0.9);
  } else if (scene.viewport === 'medium') {
    scene.tag('any Linux host · VPS · bare metal', cx + 28, cy + scene.px(46), alpha * region * header * 0.9);
  }
  // Compact has no room for the host tag above the rack; the beat copy carries it.
  // Ingress chip: the one port the deployment answers on. It sits at the right
  // end of the region header. It fades with the header when the strip is thin.
  scene.mono(10.5, 600);
  scene.chip(':8080 · nimbus start', cx + regionW - 28 - ingressW, cy + scene.px(26), alpha * settled * header, settled, palette.accent);

  // The racks. This host stands from the deployment chapter; the copies
  // slide out of it once the cluster chapter opens, so the reader watches
  // one server become three.
  // The endpoints sit a text row and a half under the keys, capped in world
  // units so a far zoom cannot push them out of the region.
  const dotsY = ry + RACK.h + Math.min(scene.px(74), 150);
  const rowStep = scene.px(18);
  RACKS.forEach((rack, index) => {
    const home = index === HOME_RACK;
    const show = home ? 1 : clamp(spread * 1.4);
    const x = rackX(index, spread);
    drawRack(scene, x, alpha * show, home ? settled : show * spread, home ? 'one process' : 'the same binary');
    if (!home && show <= 0.01) return;
    // Identity beneath the rack: the node's name, then its key. The key is
    // the address in the planned mesh; there is no IP to write here.
    const titled = home ? cluster : show * spread;
    const nameY = ry + RACK.h + scene.px(22);
    scene.tag(rack.name, x - RACK.w / 2, nameY, alpha * titled * richText, palette.accentText);
    scene.mono(11);
    const nameW = scene.measure(rack.name);
    scene.tag(rack.note, x - RACK.w / 2 + nameW + scene.px(10), nameY, alpha * titled * richText, palette.muted);
    scene.tag(rack.key, x - RACK.w / 2, nameY + scene.px(18), alpha * titled * mesh * richText, palette.muted);
    // The role, in a lower slot: who owns this request's tenant, who joined
    // as a learner and was promoted, who takes ingress as a relay.
    const roleY = rackSlotY(4);
    scene.chip(rack.role, x, roleY, alpha * roles * richText, home ? 1 : 0.5, palette.accent, 'center', RACK.w - 20);
    // The endpoint beneath the rack: where the mesh attaches.
    ctx.beginPath();
    ctx.arc(x, dotsY, scene.px(3.5), 0, Math.PI * 2);
    ctx.fillStyle = rgba(palette.accent, alpha * titled * mesh);
    ctx.fill();
  });

  // The mesh: QUIC lanes between the endpoints, direct first. The wide arc
  // is the relay path, taken when a hole punch fails. Packets ride the
  // lanes in the ambient frame and hold still in a static one.
  if (mesh > 0.01 && spread > 0.01) {
    const xs = RACKS.map((_, index) => rackX(index, spread));
    const arcs = [
      { from: 1, to: 0, depth: 120, label: 'QUIC · direct' },
      { from: 0, to: 2, depth: 120, label: 'QUIC · direct' },
      { from: 1, to: 2, depth: 260, label: 'relay · TCP/443 · when a hole punch fails' },
    ];
    arcs.forEach((arc, index) => {
      const x0 = xs[arc.from];
      const x1 = xs[arc.to];
      const draw = clamp(mesh * 1.5 - index * 0.25);
      if (draw <= 0.01) return;
      const midX = (x0 + x1) / 2;
      const controlY = dotsY + arc.depth;
      const point = (t: number) => ({
        x: (1 - t) * (1 - t) * x0 + 2 * (1 - t) * t * midX + t * t * x1,
        y: (1 - t) * (1 - t) * dotsY + 2 * (1 - t) * t * controlY + t * t * dotsY,
      });
      ctx.beginPath();
      ctx.moveTo(x0, dotsY);
      const end = point(draw);
      // Draw the arc up to `draw` by stepping the curve.
      const steps = 24;
      for (let step = 1; step <= steps; step += 1) {
        const p = point((step / steps) * draw);
        ctx.lineTo(p.x, p.y);
      }
      ctx.lineTo(end.x, end.y);
      ctx.lineWidth = scene.px(1.4);
      ctx.strokeStyle = rgba(palette.accent, alpha * cluster * (index === 2 ? 0.4 : 0.6));
      ctx.setLineDash(index === 2 ? [scene.px(6), scene.px(8)] : []);
      ctx.stroke();
      ctx.setLineDash([]);
      const bottom = point(0.5);
      scene.tag(arc.label, bottom.x, bottom.y + scene.px(20), alpha * cluster * draw * richText, index === 2 ? palette.muted : palette.accentText, 'center');
      // Packets: one each way on the direct lanes, one on the relay path.
      const phases = index === 2 ? [0.3] : [0.15, 0.65];
      phases.forEach((phase, packet) => {
        const t = ambient ? (phase + time * 0.00012 * (packet === 1 ? -1 : 1) + 10) % 1 : phase;
        const p = point(t);
        ctx.beginPath();
        ctx.arc(p.x, p.y, scene.px(2.6), 0, Math.PI * 2);
        ctx.fillStyle = rgba(palette.accent, alpha * cluster * packets * draw);
        ctx.fill();
      });
    });
  }

  // The board title under the region, the roadmap chip beside it, then what
  // the mesh carries.
  const footY = cy + regionH + scene.px(30);
  scene.mono(12, 620);
  scene.text('CLUSTER', cx + 28, footY, palette.ink, alpha * cluster * spread * far);
  const titleW = scene.measure('CLUSTER');
  scene.chip('planned · not shipped', cx + 28 + titleW + scene.px(12), footY, alpha * cluster * spread * richText, 0.2, palette.accent);
  const notes = [
    ['identity: Ed25519 key · not an IP address', 'join: token → learner → promote → voter'],
    ['QUIC mesh · UDP/7842 · direct first', 'openraft: one owner node per tenant'],
    ['relay over TCP/443 when a hole punch fails', 'blobs: bundles · OCI layers · snapshots'],
  ];
  const notesY = footY + scene.px(26);
  // Two columns when the region can hold them side by side; one column of six
  // rows when a fitted still squeezes the region narrower than the text.
  scene.mono(11);
  const leftW = Math.max(...notes.map((row) => scene.measure(row[0])));
  const rightW = Math.max(...notes.map((row) => scene.measure(row[1])));
  const rightX = Math.max(cx + regionW / 2 + 20, cx + 28 + leftW + scene.px(28));
  const twoColumns = rightX + rightW <= cx + regionW + scene.px(40);
  notes.forEach((row, index) => {
    scene.tag(row[0], cx + 28, notesY + rowStep * index, alpha * roles * richText, index < 2 ? palette.accentText : palette.muted);
    const rowIndex = twoColumns ? index : index + notes.length;
    scene.tag(row[1], twoColumns ? rightX : cx + 28, notesY + rowStep * rowIndex, alpha * roles * richText, palette.muted);
  });
  // Deploy arrives from the laptop below: a dashed lane up the cable, so the
  // descent reads as a relationship and not only a change of scene.
  const deploy = alpha * visibilityWindow(progress, 0.8828, 1.14, 0.0285);
  if (deploy > 0.01) {
    scene.tag('nimbus deploy --dry-run ↑', LAPTOP.x + scene.px(14), cy + regionH + scene.px(130), deploy * smoothstep(range(progress, 0.8909, 0.9074)), palette.muted);
  }
}

// The cable: from the foot of the region straight down to the laptop. In the
// operator chapter it is the planned path an operator's CLI takes into the
// mesh: it dials a node by key, through NAT, and never joins as a member.
function drawDescent(scene: Scene, progress: number) {
  const alpha = visibilityWindow(progress, 0.8828, 1.14, 0.0285);
  if (alpha <= 0.01) return;
  const { ctx, palette } = scene;
  const x = LAPTOP.x;
  // On a phone the copy band owns the top of the frame, so the cable shows
  // only its last stretch above the laptop.
  const y1 = LAPTOP.y - LAPTOP.h / 2 - 60;
  const y0 = scene.viewport === 'compact' ? y1 - 160 : CLOUD.y + CLOUD.h / 2 + CLOUD.spreadH + 8;
  if (!scene.inView(x - 400, x + 400, 200, y0, y1)) return;
  const grow = smoothstep(range(progress, 0.8876, 0.908));
  ctx.beginPath();
  ctx.moveTo(x, y0);
  ctx.lineTo(x, mix(y0, y1, grow));
  ctx.lineWidth = scene.px(1.4);
  ctx.strokeStyle = rgba(palette.accent, alpha * 0.55);
  ctx.setLineDash([scene.px(6), scene.px(8)]);
  ctx.stroke();
  ctx.setLineDash([]);
  // The cable always carries something: down from the region, up from the
  // laptop.
  scene.lineTraffic(x, y0, x, mix(y0, y1, grow), palette.accent, alpha * grow * 0.9, { count: 2, speed: 1.4, phase: 0.1 });
  scene.lineTraffic(x, y0, x, mix(y0, y1, grow), palette.accent, alpha * grow * 0.9, { count: 1, speed: 1.4, phase: 0.6, reverse: true });
  scene.textAlpha = scene.detail;
  // The operator's path up the cable, then what the cable always carries.
  // Wide frames keep the tags left of the cable, clear of the operator card;
  // narrower frames put them right of it, clear of the copy column, unless
  // the frame's right edge is too close for the longest tag.
  const operator = visibilityWindow(progress, 0.9101, 0.9556, 0.0104);
  const dials = smoothstep(range(progress, 0.9163, 0.9245));
  const beside = scene.viewport === 'wide' || scene.viewRight - x < scene.px(250);
  const tagX = beside ? x - scene.px(16) : x + scene.px(16);
  const align = beside ? 'right' : 'left';
  // Compact frames have one row of room above the lid: the operator tag takes
  // the place of 'the same binary' while the operator chapter runs.
  const compact = scene.viewport === 'compact';
  const tagY = compact ? y1 - scene.px(30) : beside ? 1120 : y1 - scene.px(96);
  scene.tag('dials any node by its key ↑', tagX, tagY, alpha * operator * dials, palette.accentText, align);
  if (!compact) scene.tag('through NAT · encrypted · planned', tagX, tagY + scene.px(18), alpha * operator * dials, palette.muted, align);
  scene.tag('the same binary', tagX, y1 - scene.px(30), alpha * grow * (compact ? 1 - operator * dials : 1), palette.muted, align);
  scene.textAlpha = 1;
}

// The operator key: the planned identity an operator's CLI dials the mesh
// with. On a wide frame it stands right of the laptop lid; narrower frames
// put it under the laptop caption.
function drawOperator(scene: Scene, progress: number) {
  const alpha = visibilityWindow(progress, 0.9042, 0.9556, 0.0114);
  if (alpha <= 0.01) return;
  const { palette } = scene;
  const { far, richText } = scene;
  const beside = scene.viewport === 'wide';
  const captionRows = scene.viewport === 'compact' ? 2 : 1;
  const left = beside ? OPERATOR.x - OPERATOR.w / 2 : LAPTOP.x - LAPTOP.w / 2 - 40;
  const top = beside ? OPERATOR.top : LAPTOP.y + LAPTOP.h / 2 + 32 + scene.px(40 + 20 * captionRows);
  const cardW = Math.max(OPERATOR.w, scene.px(258));
  const cardX = left + cardW / 2;
  const cardH = mix(scene.px(38), scene.px(112), scene.rich);
  if (!scene.inView(left, left + cardW, 200, top, top + cardH + 200)) return;
  const card = smoothstep(range(progress, 0.9111, 0.9203));
  const key = smoothstep(range(progress, 0.9163, 0.9245));
  const scopes = smoothstep(range(progress, 0.9203, 0.9285));
  const never = smoothstep(range(progress, 0.9245, 0.9327));
  scene.panel(cardX, top + cardH / 2, cardW, cardH, alpha * card, 0.3, palette.accent);
  scene.mono(11.5, 620);
  scene.text('OPERATOR KEY', left + 16, top + scene.px(20), palette.ink, alpha * card * far);
  const titleW = scene.measure('OPERATOR KEY');
  scene.chip('planned', left + 16 + titleW + scene.px(10), top + scene.px(20), alpha * card * richText, 0.2, palette.accent);
  scene.mono(10.5);
  scene.text('Ed25519 · in a committed roster', left + 16, top + scene.px(40), palette.muted, alpha * key * richText);
  scene.text('scopes: admin · read-only · forward', left + 16, top + scene.px(58), palette.muted, alpha * scopes * richText);
  scene.text('ALPN admin/1 · op-forward/1 only', left + 16, top + scene.px(76), palette.muted, alpha * scopes * richText);
  scene.text('never a voter · never a learner', left + 16, top + scene.px(94), palette.accentText, alpha * never * richText);
  // The key belongs to the CLI on the laptop: a lane from the lid across to
  // the card, so the card reads as the laptop's identity in the mesh.
  if (beside) {
    const lidRight = LAPTOP.x + LAPTOP.w / 2;
    const laneY = top + scene.px(20);
    scene.line(lidRight, laneY, mix(lidRight, left, key), laneY, palette.accent, alpha * key * 0.45, 1.2, [5, 7]);
  }
  scene.tag('the CLI is an endpoint too', left, top + cardH + scene.px(18), alpha * key * richText, palette.muted);
}

// The operator's shell, typed first: the planned admin commands, with the
// members the mesh answers with. Keys stand where addresses would.
const operatorLines = ['$ nimbus cluster members', '  A  7f3a…c21e  owner: demo', '  B  9b12…e07d  voter', '  C  4e88…a3f1  relay', '$ nimbus forward …'];

// Your laptop: the screen shows the operator's shell, then the quickstart.
function drawLaptop(scene: Scene, progress: number) {
  const alpha = visibilityWindow(progress, 0.895, 1.14, 0.012);
  if (alpha <= 0.01) return;
  const { ctx, palette } = scene;
  const sx = LAPTOP.x - LAPTOP.w / 2;
  const sy = LAPTOP.y - LAPTOP.h / 2;
  if (!scene.inView(sx, sx + LAPTOP.w, 200, sy, sy + LAPTOP.h + 80)) return;
  const open = smoothstep(range(progress, 0.902, 0.914));

  // Base first, then the lid rises into place.
  const baseY = sy + LAPTOP.h + 10;
  ctx.beginPath();
  ctx.moveTo(sx - 60, baseY);
  ctx.lineTo(sx + LAPTOP.w + 60, baseY);
  ctx.lineTo(sx + LAPTOP.w + 40, baseY + 22);
  ctx.lineTo(sx - 40, baseY + 22);
  ctx.closePath();
  ctx.fillStyle = rgba(mixRgb(palette.background, palette.ink, 0.12), alpha);
  ctx.fill();
  ctx.lineWidth = scene.px(1);
  ctx.strokeStyle = rgba(palette.ink, alpha * 0.3);
  ctx.stroke();

  ctx.save();
  ctx.translate(LAPTOP.x, baseY);
  ctx.scale(1, mix(0.12, 1, open));
  ctx.translate(-LAPTOP.x, -baseY);
  scene.panel(LAPTOP.x, LAPTOP.y + 5, LAPTOP.w, LAPTOP.h + 10, alpha, 0.35, palette.accent);
  // Screen surface.
  scene.roundRect(sx + 12, sy + 12, LAPTOP.w - 24, LAPTOP.h - 14, 8);
  ctx.fillStyle = rgba(night(), alpha * 0.85);
  ctx.fill();
  ctx.restore();

  scene.mono(11, 620);
  const captionY = baseY + 22 + scene.px(26);
  scene.text('LAPTOP', sx - 40, captionY, palette.ink, alpha);
  const labelW = scene.measure('LAPTOP');
  if (scene.viewport === 'compact') {
    scene.tag('macOS · Linux · the same binary', sx - 40, captionY + scene.px(20), alpha * 0.9);
  } else {
    scene.tag('macOS · Linux · the same binary', sx - 40 + labelW + scene.px(12), captionY, alpha * 0.9);
  }

  // The terminal on the screen. Lines keep a screen-pixel floor between them
  // so the minimum font size cannot stack them. The operator's shell types
  // first; it clears for the quickstart when the last chapter opens.
  const termAlpha = alpha * open;
  if (termAlpha <= 0.01) return;
  ctx.save();
  scene.roundRect(sx + 12, sy + 12, LAPTOP.w - 24, LAPTOP.h - 14, 8);
  ctx.clip();
  scene.tag('terminal', sx + 30, sy + 34, termAlpha, mixRgb(palette.muted, inkNight(), 0.4));
  const clear = smoothstep(range(progress, 0.9545, 0.9606));
  const scripts = [
    { lines: operatorLines, typed: smoothstep(range(progress, 0.916, 0.932)), show: 1 - clear },
    { lines: terminalLines, typed: smoothstep(range(progress, 0.963, 0.978)), show: clear },
  ];
  const lineH = Math.max(30, scene.px(14));
  scripts.forEach((script) => {
    if (script.show <= 0.01) return;
    const scriptAlpha = termAlpha * script.show;
    scene.mono(12.5, 500);
    // A line types in: the clip follows the caret across it.
    script.lines.forEach((line, index) => {
      const reveal = clamp(script.typed * script.lines.length - index);
      if (reveal <= 0) return;
      const isOutput = line.startsWith(' ');
      const lineY = sy + 70 + index * lineH;
      ctx.save();
      ctx.beginPath();
      ctx.rect(sx + 30, lineY - lineH / 2, scene.measure(line) * reveal + scene.px(2), lineH);
      ctx.clip();
      scene.text(line, sx + 30, lineY, isOutput ? WHITE : inkNight(), scriptAlpha * (isOutput ? 1 : 0.92));
      ctx.restore();
    });
    // Cursor blinks at the end of the last revealed line.
    const lineIndex = Math.min(script.lines.length - 1, Math.floor(script.typed * script.lines.length));
    const cursorY = sy + 70 + lineIndex * lineH;
    const lineW = scene.measure(script.lines[lineIndex]) * clamp(script.typed * script.lines.length - lineIndex);
    // Solid while a line types; a steady blink once the script has typed.
    const blink = scene.ambient && script.typed >= 1 ? (Math.floor(scene.time / 530) % 2 === 0 ? 1 : 0.12) : 1;
    ctx.fillStyle = rgba(palette.accent, scriptAlpha * 0.9 * blink);
    ctx.fillRect(sx + 30 + lineW + scene.px(4), cursorY - scene.px(7), scene.px(7), scene.px(14));
  });
  ctx.restore();
}

function drawRequest(scene: Scene, progress: number, time: number, route: Route, layout: Layout, ambient: boolean) {
  const point = pointOnRoute(route, progress);
  const alpha = visibilityWindow(progress, -0.95, 1.14, 0.019);
  if (alpha <= 0.01) return;
  const { ctx, palette } = scene;
  if (!scene.inView(point.x, point.x, 200, point.y, point.y)) return;
  const pulse = ambient ? 0.5 + 0.5 * Math.sin(time * 0.004) : 0.5;
  const size = scene.px(7) + scene.px(1.2) * pulse;

  // Trail: where the request was a moment ago. Faster travel leaves a longer
  // trail, so a hold reads as rest and a move reads as motion.
  for (let index = 1; index <= 14; index += 1) {
    const back = pointOnRoute(route, progress - index * 0.003);
    const fade = 1 - index / 15;
    ctx.beginPath();
    ctx.arc(back.x, back.y, scene.px(2.4) * fade, 0, Math.PI * 2);
    ctx.fillStyle = rgba(palette.accent, alpha * fade * 0.45);
    ctx.fill();
  }

  const glow = ctx.createRadialGradient(point.x, point.y, 0, point.x, point.y, scene.px(44));
  glow.addColorStop(0, rgba(palette.accent, alpha * 0.45));
  glow.addColorStop(1, rgba(palette.accent, 0));
  ctx.fillStyle = glow;
  ctx.fillRect(point.x - scene.px(44), point.y - scene.px(44), scene.px(88), scene.px(88));

  ctx.save();
  ctx.translate(point.x, point.y);
  ctx.rotate(Math.PI / 4);
  ctx.fillStyle = rgba(palette.accent, alpha);
  ctx.fillRect(-size, -size, size * 2, size * 2);
  ctx.strokeStyle = rgba(palette.background, alpha);
  ctx.lineWidth = scene.px(1.5);
  ctx.strokeRect(-size, -size, size * 2, size * 2);
  ctx.restore();

  // State label on a small pill so it stays legible over whatever the request
  // is passing through. It sits below the top door while the request passes
  // it (the heading sits above), flips to the left near the right edge of
  // the view, and fades when the request itself has left the frame. Where
  // that default would land on a neighbouring label, the pill moves to a
  // clear lane: above the engine and the runtime boxes while the request
  // works inside them, into the gap between two doors when the copy column
  // sits close to the doors (phone, tablet), above the sealed transaction
  // and the publish box, under the storage tile, beside the files panel,
  // under the request beside the service card, and over the agent card. It
  // rests while the camera frames the whole binary and while the request
  // sits in the rack; the status bar carries the state there.
  const label = requestStateFor(progress);
  const screenX = scene.screenX(point.x);
  const rest = visibilityWindow(progress, 0.7015, 0.7541, 0.019);
  const landed = smoothstep(range(progress, 0.9157, 0.9254));
  const edgeFade = (1 - smoothstep(range(screenX, scene.width - 8, scene.width + 40))) * smoothstep(range(screenX, -40, 8));
  const labelAlpha = alpha * edgeFade * (1 - rest) * (1 - landed);
  if (labelAlpha <= 0.01) return;
  scene.mono(10.5, 620);
  const textWidth = scene.measure(label);
  const padX = scene.px(8);
  const pillH = scene.px(20);
  // On a phone the app card sits under the copy's footer, so the pill rides
  // above the card while the request is born inside it.
  const appLane = scene.viewport === 'compact' ? visibilityWindow(progress, -0.95, 0.0381, 0.019) : 0;
  const engineLane = visibilityWindow(progress, 0.1316, 0.1574, 0.019);
  const doorLane = scene.viewport === 'wide' ? 0 : visibilityWindow(progress, 0.0665, 0.0947, 0.019);
  // Inside the runtime box, through the functions and the Node chapters, the
  // pill rides above the box, clear of the isolate card and the direct lane
  // over it.
  const runtimeLane = visibilityWindow(progress, 0.1542, 0.2516, 0.0076);
  // At the tenant door the pill sits level with the request, to its right,
  // clear of the door card below it.
  const tenantDoorLane = visibilityWindow(progress, 0.4657, 0.5608, 0.0095);
  // Beside the service card the pill sits under the request, centred, clear
  // of the card and of the lease that drops past it.
  const serviceLane = Math.max(visibilityWindow(progress, 0.5693, 0.6013, 0.0095), visibilityWindow(progress, 0.6527, 0.6692, 0.0095));
  // On the workloads board the pill sits under the request, centred, in
  // the clear space right of the compose card.
  const workloadsLane = visibilityWindow(progress, 0.6067, 0.6467, 0.0095);
  // Through authorize and validate the pill rides above the request,
  // centred, so it never covers the lanes between the boxes; at the sealed
  // transaction it rides above the box's name.
  // The commit pill holds through the chapter mid; it hands off to the
  // transaction lane at the edge, not before.
  const commitLane = smoothstep(range(progress, 0.2528, 0.2623)) * (1 - smoothstep(range(progress, 0.276, 0.2789)));
  const txnLane = visibilityWindow(progress, 0.2789, 0.2996, 0.0029);
  // At publish the pill rides above the box; docked at the subscribers card
  // it sits left of the request and above the lane, clear of the card.
  const publishLane = visibilityWindow(progress, 0.2996, 0.3071, 0.0057);
  const fanLane = visibilityWindow(progress, 0.3071, 0.3471, 0.0057);
  // On the storage shelf the pill sits under the default engine, clear of
  // its base and of the row note beside it.
  const shelfLane = visibilityWindow(progress, 0.3539, 0.3939, 0.019);
  // Beside the files panel the pill sits to the left of the request, clear
  // of the panel heading.
  const filesLane = visibilityWindow(progress, 0.4083, 0.4457, 0.019);
  // Inside the sandbox the pill rides above the agent card, centred, clear of
  // the column of facts beside it.
  const sandboxLane = visibilityWindow(progress, 0.6811, 0.7983, 0.0095);
  // Where the camera is too far out for the box to carry detail text, the
  // pill would sit on the box title, and the status bar already carries the
  // state, so the pill stays out.
  // On a phone the runtime box sits against the copy's footer, so the pill
  // stays out there too, and through the agent plane, where the door, the
  // service and the box carry no detail text at the phone's zoom.
  // The stay-out holds until the request has left the box: the pill would
  // otherwise return over the box title during the pullback.
  const phoneRuntime = scene.viewport === 'compact' ? visibilityWindow(progress, 0.1542, 0.2485, 0.019) : 0;
  const phoneAgent = scene.viewport === 'compact' ? visibilityWindow(progress, 0.4527, 0.8323, 0.019) : 0;
  // On a tablet the request rests near the left frame edge beside the files
  // panel: the flipped pill would be pushed back over the panel heading.
  const tabletFiles = scene.viewport === 'medium' ? filesLane : 0;
  const farBox = visibilityWindow(progress, 0.6811, 0.9023, 0.019);
  const phoneRest = Math.max(Math.max(sandboxLane, phoneRuntime, phoneAgent, farBox, workloadsLane) * (1 - scene.rich), tabletFiles);
  const pillW = textWidth + padX * 2;
  const overflow = scene.screenX(point.x + scene.px(16) + pillW) > scene.width - 10;
  const flip = screenX > scene.width * 0.72 || overflow || doorLane > 0.5 || filesLane > 0.5;
  const beside = smoothstep(range(progress, 0.0415, 0.0528));
  let pillX = flip ? point.x - scene.px(16) - pillW : point.x + scene.px(16);
  pillX = mix(pillX, point.x - pillW / 2, Math.max(commitLane, txnLane, publishLane, shelfLane, sandboxLane, serviceLane));
  pillX = mix(pillX, point.x - scene.px(10) - pillW, fanLane);
  pillX = mix(pillX, point.x - pillW / 2, filesLane * (scene.viewport === 'compact' ? 1 : 0));
  pillX = mix(pillX, RUNTIME.x - pillW / 2, runtimeLane);
  pillX = mix(pillX, APP.x - pillW / 2, appLane);
  pillX = mix(pillX, point.x - pillW / 2, workloadsLane);
  // Above the agent card the centred pill would reach the box title at a
  // short frame's zoom, so it keeps to the right of the title.
  scene.mono(12, 620);
  const boxTitleRight = SANDBOX.x - SANDBOX.w / 2 + 22 + scene.measure('SANDBOX') + scene.px(10);
  scene.mono(10.5, 620);
  pillX = mix(pillX, Math.max(pillX, boxTitleRight), sandboxLane);
  // A rest point near the frame edge keeps its pill inside the stage.
  const pillInset = scene.px(14);
  pillX = clamp(pillX, scene.viewLeft + pillInset, scene.viewRight - pillInset - pillW);
  let pillY = point.y + (DOOR_H / 2 + scene.px(14)) * beside;
  pillY = mix(pillY, ENGINE.y - ENGINE.h / 2 - scene.px(14), engineLane);
  pillY = mix(pillY, -1.5 * layout.doorGap, doorLane);
  pillY = mix(pillY, RUNTIME.y - RUNTIME.h / 2 - scene.px(14), runtimeLane);
  pillY = mix(pillY, point.y + scene.px(60), serviceLane);
  pillY = mix(pillY, point.y, tenantDoorLane);
  pillY = mix(pillY, point.y + scene.px(50), workloadsLane);
  pillY = mix(pillY, point.y - (AUTHORIZE.h / 2 + scene.px(34)), commitLane);
  pillY = mix(pillY, -(TXN.h / 2 + 34) - scene.px(42), txnLane);
  pillY = mix(pillY, PUBLISH.y - PUBLISH.h / 2 - scene.px(14), publishLane);
  pillY = mix(pillY, point.y - scene.px(24), fanLane);
  pillY = mix(pillY, ENGINE_TILE.embeddedY + ENGINE_TILE.h / 2 + scene.px(46), shelfLane);
  pillY = mix(pillY, point.y - (scene.viewport === 'compact' ? scene.px(38) : 0), filesLane);
  pillY = mix(pillY, AGENT.y - AGENT.h / 2 - scene.px(14), sandboxLane);
  pillY = mix(pillY, APP.y - APP.h / 2 - scene.px(14), appLane);
  const pillAlpha = labelAlpha * (1 - phoneRest);
  if (pillAlpha <= 0.01) return;
  scene.roundRect(pillX, pillY - pillH / 2, pillW, pillH, pillH / 2);
  ctx.fillStyle = rgba(palette.background, pillAlpha * 0.86);
  ctx.fill();
  ctx.lineWidth = scene.px(1);
  ctx.strokeStyle = rgba(palette.accent, pillAlpha * 0.5);
  ctx.stroke();
  scene.text(label, pillX + padX, pillY, palette.accentText, pillAlpha * 0.98);
}

// Storyboard stills that carry the detail level: the runtime with the Node
// target beneath it, the backends shelf, the files panel, and the four agent
// frames, the box on its host ground, its proxy, the service above it and the
// sessions above that, each centred on its composition. The zoom sits just
// above the detail band, so the labels render. The tenants still frames the
// whole process with its deck instead.
// `narrow` frames a world box for the phone-width stills, where the fitted
// zoom would shrink the composition under its label floors.
const RICH_STILLS: { from: number; to: number; x: number; y: number; zoom: number; fit?: number; narrow?: { y: number; w: number; h: number } }[] = [
  { from: 0.2023, to: 0.2422, x: 2740, y: 150, zoom: 0.58 },
  // The two storage stills reach below the process edge to the database and
  // the bucket, so they name the world height to fit and let the zoom follow
  // the frame.
  { from: 0.3539, to: 0.3939, x: 4300, y: 1060, zoom: 0.53, fit: 1000 },
  { from: 0.4045, to: 0.4444, x: FILES.x, y: 1000, zoom: 0.53, fit: 1040 },
  // The box still reaches from the tenant door to the host ground, so it
  // names its world height too.
  { from: 0.455, to: 0.495, x: HOST.x, y: 1430, zoom: 0.53, fit: 1060 },
  { from: 0.5056, to: 0.5455, x: 6450, y: 1385, zoom: 0.5, fit: 1050 },
  // The service still reaches from the ingress facts to the top of the box
  // the lanes land in; the session still from the row of sessions down to it.
  { from: 0.5561, to: 0.5961, x: 6455, y: 900, zoom: 0.53 },
  { from: 0.6067, to: 0.6467, x: WORKLOADS.x, y: WORKLOADS.y, zoom: 0.53 },
  { from: 0.6573, to: 0.6972, x: 6250, y: 840, zoom: 0.53, fit: 1400 },
  { from: 0.7584, to: 0.7983, x: 4000, y: 380, zoom: 0.09 },
  { from: 0.8089, to: 0.8489, x: 8300, y: -1300, zoom: 0.6, fit: 780 },
  { from: 0.8595, to: 0.8995, x: 8300, y: -990, zoom: 0.5, fit: 1300, narrow: { y: -1150, w: 1340, h: 1040 } },
  { from: 0.9101, to: 0.95, x: 8300, y: 1600, zoom: 0.6, fit: 1000, narrow: { y: 1560, w: 620, h: 440 } },
  { from: 0.9606, to: 1, x: 8300, y: 1540, zoom: 0.6, fit: 720, narrow: { y: 1560, w: 620, h: 440 } },
];

// The stage chrome in screen pixels: the nav band above and the status rail
// below, per viewport (see globals.css). The world is framed between them.
const CHROME: Record<Viewport, { top: number; bottom: number }> = {
  wide: { top: 100, bottom: 76 },
  medium: { top: 84, bottom: 84 },
  compact: { top: 66, bottom: 68 },
};

export function drawFrame(ctx: CanvasRenderingContext2D, options: FrameOptions) {
  const { width, height, dpr, progress, time, viewport, ambient, centered } = options;
  const camera = interpolateCamera(progress, viewport);
  const richStill = centered && width >= 600 ? RICH_STILLS.find((still) => progress >= still.from && progress <= still.to) : undefined;
  if (richStill) {
    // A still wide enough for the detail level frames the primitives at the
    // zoom the desktop chapter uses, centred on the composition.
    camera.x = richStill.x;
    camera.y = richStill.y;
    camera.zoom = (richStill.fit ? Math.min(richStill.zoom, (height - 48) / richStill.fit) : richStill.zoom) * 0.92;
    camera.anchorX = 0.5;
    camera.anchorY = 0.5;
  } else if (centered) {
    // Storyboard stills: the phone composition, centred, scaled to the still.
    // The phone keyframes frame a band about 390 wide and 420 tall. A chapter
    // with a narrow box frames that box instead.
    const narrowStill = RICH_STILLS.find((still) => still.narrow && progress >= still.from && progress <= still.to);
    camera.anchorX = 0.5;
    camera.anchorY = 0.5;
    if (narrowStill?.narrow) {
      camera.x = narrowStill.x;
      camera.y = narrowStill.narrow.y;
      camera.zoom = Math.min(width / narrowStill.narrow.w, height / narrowStill.narrow.h) * 0.92;
    } else {
      camera.zoom *= clamp(Math.min(width / 390, height / 420), 0.35, 1) * 0.88;
    }
  }
  // The chrome band: the nav at the top and the status rail at the bottom.
  // The world is framed in the band between them, so no composition sits
  // under either. Storyboard stills have no chrome.
  const chrome = centered ? { top: 0, bottom: 0 } : CHROME[viewport];
  const safeTop = chrome.top;
  const safeH = height - chrome.top - chrome.bottom;
  // Desktop keyframes are composed for the 1440×900 stage's band and tablet
  // keyframes for 1024×768's. A shorter or narrower stage pulls the camera
  // back a little so the tallest and widest compositions (the five doors, the
  // binary, the cloud region) stay inside the band and clear of the copy.
  // Small labels fade by the composed zoom, so a stage that pulls the camera
  // back to fit keeps the detail its chapter was composed with.
  const lodZoom = camera.zoom;
  if (viewport !== 'compact') {
    const composedH = 900 - CHROME.wide.top - CHROME.wide.bottom;
    const composedTabletH = 768 - CHROME.medium.top - CHROME.medium.bottom;
    camera.zoom *= clamp(safeH / (viewport === 'wide' ? composedH : composedTabletH), 0.8, 1) * clamp(width / (viewport === 'wide' ? 1440 : 1024), 0.8, 1);
  } else if (!centered) {
    // Phone keyframes are composed for 390×844. The copy block above the world
    // has a fixed height, so the world scales with the room left below it and
    // its anchor drops towards the centre of that room. Storyboard stills are
    // centred and keep their composed zoom.
    const room = clamp((safeH - 366) / 344, 0.7, 1);
    camera.zoom *= room;
    camera.anchorY += (1 - room) * 0.08;
  }
  const palette = paletteFor(progress);
  const layout = layouts[viewport];

  drawBackdrop(ctx, options, camera, palette);

  ctx.setTransform(
    dpr * camera.zoom,
    0,
    0,
    dpr * camera.zoom,
    dpr * (width * camera.anchorX - camera.x * camera.zoom),
    dpr * (safeTop + safeH * camera.anchorY - camera.y * camera.zoom),
  );

  // A fitted still may drop below the detail band; it keeps the detail text.
  // A still composed at the desktop detail zoom keeps that detail level even
  // where the frame pulls the zoom back to fit its world height.
  const scene = new Scene(ctx, { ...options, lodZoom, forceRich: options.forceRich || (richStill !== undefined && richStill.zoom >= 0.5) }, camera, palette);
  drawBinary(scene, progress);
  drawTenantSheets(scene, progress);
  // The interior steps back while the workloads board is up, so the host
  // and the service card behind the copy column do not compete with it.
  ctx.globalAlpha = tenantDeck(scene, progress).dim * (1 - 0.7 * visibilityWindow(progress, 0.6067, 0.6467, 0.0104));
  drawApp(scene, progress, layout);
  drawDoors(scene, progress, layout);
  drawEngine(scene, progress);
  drawRuntime(scene, progress);
  drawCommit(scene, progress);
  drawBackends(scene, progress);
  drawFiles(scene, progress);
  drawSandbox(scene, progress);
  drawServices(scene, progress);
  ctx.globalAlpha = 1;
  drawWorkloads(scene, progress);
  drawTenants(scene, progress);
  drawCloud(scene, progress, time, ambient);
  drawDescent(scene, progress);
  drawOperator(scene, progress);
  drawLaptop(scene, progress);
  drawRequest(scene, progress, time, routeTrack(routeFor(layout)), layout, ambient);

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  // The chrome mask: the nav band and the status rail lie over the stage, so
  // whatever a tall composition leaves under them fades into the background
  // before it reaches them. Storyboard stills have no chrome.
  if (!centered) {
    const topMask = ctx.createLinearGradient(0, chrome.top - 30, 0, chrome.top + 34);
    topMask.addColorStop(0, rgba(palette.background, 1));
    topMask.addColorStop(1, rgba(palette.background, 0));
    ctx.fillStyle = topMask;
    ctx.fillRect(0, 0, width, chrome.top + 34);
    const bottomMask = ctx.createLinearGradient(0, height - chrome.bottom - 34, 0, height - chrome.bottom + 30);
    bottomMask.addColorStop(0, rgba(palette.background, 0));
    bottomMask.addColorStop(1, rgba(palette.background, 1));
    ctx.fillStyle = bottomMask;
    ctx.fillRect(0, height - chrome.bottom - 34, width, chrome.bottom + 34);
  }
  return { camera, palette };
}
