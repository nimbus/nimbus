'use client';

import { type KeyboardEvent as ReactKeyboardEvent, useEffect, useId, useRef, useState } from 'react';
import '@/styles/hero.css';

// The install paths, as the quickstart and the README give them: one tab per
// way to get the binary, each ending at a running dev server where it can.
type InstallPath = { id: string; label: string; note: string; lines: string[] };
const INSTALLS: InstallPath[] = [
  {
    id: 'brew',
    label: 'Homebrew',
    note: 'macOS and Linux',
    lines: ['brew install nimbus/tap/nimbus', 'nimbus init convex my-app && cd my-app', 'nimbus dev'],
  },
  {
    id: 'script',
    label: 'install.sh',
    note: 'Linux · x86_64 and ARM64',
    lines: ['curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh', 'nimbus init convex my-app && cd my-app', 'nimbus dev'],
  },
  {
    id: 'docker',
    label: 'Docker',
    note: 'Container image',
    lines: ['docker volume create nimbus-data', 'docker run --rm -p 127.0.0.1:8080:8080 -v nimbus-data:/var/lib/nimbus ghcr.io/nimbus/nimbus:latest'],
  },
  {
    id: 'source',
    label: 'Source',
    note: 'Rust toolchain',
    lines: ['git clone https://github.com/nimbus/nimbus.git && cd nimbus', 'cargo install --path crates/nimbus-bin', 'nimbus init convex my-app && cd my-app', 'nimbus dev'],
  },
];

export function Install() {
  const [active, setActive] = useState(0);
  const [copied, setCopied] = useState(false);
  const timer = useRef(0);
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  // The block renders twice on the page, in the hero and on the last
  // chapter, so each tab list owns its ids.
  const id = useId();
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const path = INSTALLS[active];
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(path.lines.join('\n'));
    } catch {
      return;
    }
    setCopied(true);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(false), 1600);
  };
  const select = (index: number) => {
    const next = (index + INSTALLS.length) % INSTALLS.length;
    setActive(next);
    setCopied(false);
    tabs.current[next]?.focus();
  };
  // Arrow keys move along the tabs, as a tab list does.
  const onKey = (event: ReactKeyboardEvent<HTMLButtonElement>, index: number) => {
    if (event.key === 'ArrowRight') select(index + 1);
    else if (event.key === 'ArrowLeft') select(index - 1);
    else if (event.key === 'Home') select(0);
    else if (event.key === 'End') select(INSTALLS.length - 1);
    else return;
    event.preventDefault();
  };
  return (
    <div className="hero-install">
      <div className="hero-install-head">
        <div className="hero-install-tabs" role="tablist" aria-label="Install method">
          {INSTALLS.map((option, index) => (
            <button
              key={option.id}
              ref={(node) => {
                tabs.current[index] = node;
              }}
              type="button"
              role="tab"
              id={`${id}-tab-${option.id}`}
              aria-selected={index === active}
              aria-controls={`${id}-panel-${option.id}`}
              tabIndex={index === active ? 0 : -1}
              onClick={() => select(index)}
              onKeyDown={(event) => onKey(event, index)}
            >
              {option.label}
            </button>
          ))}
        </div>
        <button type="button" className="hero-install-copy" onClick={copy} aria-live="polite">
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <div className="hero-install-note" aria-hidden="true">
        shell · {path.note}
      </div>
      <pre role="tabpanel" id={`${id}-panel-${path.id}`} aria-labelledby={`${id}-tab-${path.id}`} aria-label={`${path.label} install commands`}>
        <code>
          {path.lines.map((line) => (
            <span className="line" key={line}>
              <i aria-hidden="true">$ </i>
              {line}
            </span>
          ))}
        </code>
      </pre>
    </div>
  );
}
