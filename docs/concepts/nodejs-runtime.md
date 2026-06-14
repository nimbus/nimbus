---
title: How the Node runtime works
description: Why Nimbus's Node.js support is a measured, in-process compatibility contract rather than a Node process.
sidebar:
  order: 7
---

Nimbus's Node.js support is an explicit compatibility contract for function
invocation. It is not a Node process, and it is not a claim that every Node
CLI feature, process-wide API, native extension, or host tooling workflow is
available. This page explains the model; the
[Node runtime guides](/developers/runtimes/nodejs/) cover how to use it.

## One engine, two compatibility surfaces

All Nimbus functions execute in-process in the same V8-based engine. What
differs is the compatibility target a module runs against:

- The **web-standard isolate** target is the default. It exposes web
  platform APIs — `fetch`, `URL`, Web Crypto, streams — and runs every query
  and mutation, plus any action that doesn't opt out.
- The **Node compatibility targets** (Node20, Node22, Node24, Node26) add a
  Node built-in surface for action modules that opt in with `"use node"`.
  The module still runs in the same in-process engine; Nimbus provides the
  Node APIs as a compatibility layer rather than delegating to an external
  `node` binary.

Because there is no separate Node process, a Node action gets the same
invocation lifecycle, host-call path, and operational behavior as every
other function. Database operations from a Node action flow through the same
engine path as direct API calls — there is no side channel.

## Compatibility and permissions are separate axes

Selecting a Node version chooses *what JavaScript surface the code sees*. It
never chooses *what the code is allowed to touch*. Those are independent:

- The **compatibility target** (web-standard isolate, or a Node version)
  bounds the API surface.
- The **permission mode** — `Restricted`, `Standard`, or `Privileged` —
  plus fine-grained grants bound the resource surface: filesystem roots,
  network hosts, environment names, secrets, subprocess commands, and so on.

A Node target does not imply ambient host access, and a broader permission
mode does not change the JavaScript compatibility target. This is why
`process.env` access, outbound network calls, and filesystem reads behave
the same way under a Node target as anywhere else in Nimbus: they are
governed by grants, not by the word "Node".

## Why the surface is bounded

The in-process runtime is optimized for deterministic function invocation:
load a module once, invoke it repeatedly, complete awaited async work, and
surface dangling-promise diagnostics. Many Node behaviors fundamentally
conflict with that shape — child processes, worker threads, the inspector,
the REPL, `node --test`, native addons, raw TCP listeners, and persistent
writable filesystem state all assume host-level authority and process-wide
lifetime.

Rather than half-supporting these, Nimbus draws a hard boundary: host-heavy
behavior is denied in-process with a clear diagnostic, and belongs in a
service or microVM profile that actually has host authority. The denial
itself is tested — diagnostic canaries prove each boundary produces the
intended error instead of an undefined failure mode.

## Support claims are evidence-backed

Nimbus does not use runtime names as blanket compatibility claims:
supported surfaces are backed by executed upstream Node tests and real
package checks, and known gaps are recorded boundaries rather than support
claims. The
[Node compatibility reference](/reference/runtimes/node-compat/) publishes
the current contract and headline numbers.

## Packages are staged, not installed

Node packages are resolved from local `node_modules` and staged during
codegen. Runtime invocation never runs `npm install`, never fetches from the
network, and never discovers packages outside the generated bundle metadata.
This keeps invocation deterministic and keeps the supply-chain surface at
build time, where it can be inspected, rather than at runtime.

## Version targets are explicit

Each Node version is a distinct compatibility target with its own evidence:
Node24 is the default routing target, Node22 is a supported Maintenance LTS
peer, Node26 tracks the Current line, and Node20 remains selectable only as
a legacy-grace target past its end of life. The default is a routing
default, not an evidence priority — support status per version comes from
the [compatibility contract](/reference/runtimes/node-compat/), not from
which version is selected when you don't choose one.
