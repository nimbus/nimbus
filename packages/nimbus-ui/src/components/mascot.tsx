import type { CSSProperties, SVGProps } from "react";

import { useReducedMotion } from "@/hooks/use-reduced-motion";
import { cn } from "@/lib/utils";

// The mascot is the Nimbus mark: a cloud with a face, drawn in the fewest
// marks that still read as someone. The body is a union of three lobes and a
// soft base in a 120×92 box, so it holds from 16px in a tab bar up to a
// hero illustration. The face carries the state; nothing else moves.
//
// `outline` is the mark: a currentColor stroke on the panel ground, the one
// that sits next to the wordmark and in the sidebar. `solid` is the sticker:
// an amber body with an ink face, for the app icon, the first-run card and
// empty-state illustrations at 32px and above. The amber is the accent, so
// the solid variant appears only where the accent may.
export type MascotState = "idle" | "working" | "error" | "empty" | "celebrate";
export type MascotVariant = "outline" | "solid";

export const MASCOT_STATES: ReadonlyArray<MascotState> = [
  "idle",
  "working",
  "error",
  "empty",
  "celebrate",
];

export const MASCOT_VIEWBOX = "0 0 120 92";
const ASPECT = 92 / 120;
const EYE_L = 48;
const EYE_R = 72;
const EYE_Y = 52;
const MOUTH_Y = 64;

function Body({
  variant,
  strokeWidth,
}: {
  variant: MascotVariant;
  strokeWidth: number;
}) {
  const shapes = (
    <>
      <circle cx="36" cy="50" r="20" />
      <circle cx="60" cy="40" r="28" />
      <circle cx="84" cy="50" r="20" />
      <rect x="14" y="50" width="92" height="34" rx="17" />
    </>
  );
  if (variant === "solid") {
    return (
      <g data-part="body" fill="var(--accent)">
        {shapes}
      </g>
    );
  }
  // Stroke every shape, then fill the same shapes on top: the inner seams
  // vanish and only the union outline remains.
  return (
    <>
      <g
        data-part="body"
        fill="none"
        stroke="currentColor"
        strokeWidth={strokeWidth}
        strokeLinejoin="round"
      >
        {shapes}
      </g>
      <g fill="var(--bg-panel)">{shapes}</g>
    </>
  );
}

// Eyes are the only part that can move. `blink` attaches the keyframes;
// the group scales about the eye line so a blink closes the dots in place.
function DotEyes({
  ink,
  dx,
  blink,
  r,
}: {
  ink: string;
  dx: number;
  blink: boolean;
  r: number;
}) {
  const style: CSSProperties = { transformOrigin: `60px ${EYE_Y}px` };
  return (
    <g
      data-part="eyes"
      data-blink={blink ? "true" : undefined}
      className={cn(blink && "animate-blink")}
      style={style}
      fill={ink}
    >
      <circle cx={EYE_L + dx} cy={EYE_Y} r={r} />
      <circle cx={EYE_R + dx} cy={EYE_Y} r={r} />
    </g>
  );
}

function ClosedEyes({ ink, up, w }: { ink: string; up: boolean; w: number }) {
  const d = up
    ? "M42 54 q6 -7 12 0 M66 54 q6 -7 12 0"
    : "M42 51 q6 6 12 0 M66 51 q6 6 12 0";
  return (
    <path
      data-part="eyes"
      d={d}
      fill="none"
      stroke={ink}
      strokeWidth={w}
      strokeLinecap="round"
    />
  );
}

function CrossEyes({ ink, w }: { ink: string; w: number }) {
  return (
    <path
      data-part="eyes"
      d="M44 48 l8 8 M52 48 l-8 8 M68 48 l8 8 M76 48 l-8 8"
      fill="none"
      stroke={ink}
      strokeWidth={w}
      strokeLinecap="round"
    />
  );
}

function Mouth({ ink, d, w }: { ink: string; d: string; w: number }) {
  return (
    <path
      data-part="mouth"
      d={d}
      fill="none"
      stroke={ink}
      strokeWidth={w}
      strokeLinecap="round"
    />
  );
}

const SMILE = `M52 ${MOUTH_Y - 2} q8 8 16 0`;
const FLAT = "M55 65 h10";
const WOBBLE = "M52 66 q4 -4 8 0 t8 0";

// weight is the face line width in viewBox units; the dot radius follows it.
type FaceWeight = { w: number; r: number };

function Face({
  state,
  ink,
  spark,
  blink,
  weight,
}: {
  state: MascotState;
  ink: string;
  spark: string;
  blink: boolean;
  weight: FaceWeight;
}) {
  const { w, r } = weight;
  switch (state) {
    case "idle":
      return (
        <>
          <DotEyes ink={ink} dx={0} blink={blink} r={r} />
          <Mouth ink={ink} d={SMILE} w={w} />
        </>
      );
    case "working":
      return (
        <>
          <DotEyes ink={ink} dx={3} blink={blink} r={r} />
          <Mouth ink={ink} d={FLAT} w={w} />
          <g data-part="thinking" fill={spark}>
            <circle cx="96" cy="26" r="2" />
            <circle cx="104" cy="20" r="2.6" />
            <circle cx="113" cy="13" r="3.2" />
          </g>
        </>
      );
    case "error":
      return (
        <>
          <CrossEyes ink={ink} w={w} />
          <Mouth ink={ink} d={WOBBLE} w={w} />
          <path
            data-part="drop"
            d="M98 30 c0 -4 5 -10 5 -10 s5 6 5 10 a5 5 0 0 1 -10 0z"
            fill={spark}
          />
        </>
      );
    case "empty":
      return (
        <>
          <ClosedEyes ink={ink} up={false} w={w} />
          <Mouth ink={ink} d={FLAT} w={w} />
          <g
            data-part="sleep"
            fill="none"
            stroke={spark}
            strokeWidth="3"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M92 24 h8 l-8 8 h8" />
            <path d="M104 12 h6 l-6 6 h6" />
          </g>
        </>
      );
    case "celebrate":
      return (
        <>
          <ClosedEyes ink={ink} up w={w} />
          <path data-part="mouth" d="M50 62 q10 12 20 0 z" fill={ink} />
          <g data-part="sparks" fill={spark}>
            <path d="M100 14 l2 5 5 2 -5 2 -2 5 -2 -5 -5 -2 5 -2z" />
            <path d="M18 22 l1.5 4 4 1.5 -4 1.5 -1.5 4 -1.5 -4 -4 -1.5 4 -1.5z" />
            <path d="M110 40 l1 3 3 1 -3 1 -1 3 -1 -3 -3 -1 3 -1z" />
          </g>
        </>
      );
  }
}

export type MascotProps = {
  /** Rendered width in CSS pixels; the height follows the 120:92 body. */
  size?: number;
  state?: MascotState;
  variant?: MascotVariant;
  /** Accessible name. Pass `decorative` instead when the text beside the mascot already says what it says. */
  label?: string;
  decorative?: boolean;
  className?: string;
} & Omit<SVGProps<SVGSVGElement>, "width" | "height" | "children">;

export function Mascot({
  size = 24,
  state = "idle",
  variant = "outline",
  label = "Nimbus",
  decorative = false,
  className,
  ...props
}: MascotProps) {
  const reducedMotion = useReducedMotion();
  // Only open dot eyes can blink, and only when the operating system allows
  // motion. Under reduced motion the animation is absent, not paused.
  const blink = !reducedMotion && (state === "idle" || state === "working");
  const solid = variant === "solid";
  const ink = solid ? "var(--accent-ink)" : "currentColor";
  const spark = solid ? "currentColor" : "var(--accent)";
  // A 5-unit stroke is 1px at 24px, too thin to sit next to a semibold
  // wordmark. Below 40px the outline and the face thicken together so the
  // mark keeps the weight of the text beside it.
  const small = size < 40;
  const strokeWidth = small ? 7 : 5;
  const weight: FaceWeight = small ? { w: 5.5, r: 4.6 } : { w: 4, r: 3.7 };
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox={MASCOT_VIEWBOX}
      width={size}
      height={Math.round(size * ASPECT)}
      className={cn("shrink-0", className)}
      data-mascot={variant}
      data-state={state}
      role={decorative ? undefined : "img"}
      aria-label={decorative ? undefined : label}
      aria-hidden={decorative ? true : undefined}
      {...props}
    >
      <Body variant={variant} strokeWidth={strokeWidth} />
      <Face
        state={state}
        ink={ink}
        spark={spark}
        blink={blink}
        weight={weight}
      />
    </svg>
  );
}
