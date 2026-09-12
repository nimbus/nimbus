// The social card, rendered from `docs/brand/mascot/og.svg` by
// `docs/brand/mascot/render.sh`. Next does not inherit `openGraph` field by
// field: a page that declares its own `openGraph` replaces the root one
// whole, so any page that overrides the block has to repeat this image.
export const OG_IMAGE = {
  url: '/og.png',
  width: 1200,
  height: 630,
  alt: 'nimbus — your cloud backend in one binary',
} as const;
