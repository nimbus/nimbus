# Licensing

Nimbus is source-available software. The binding legal terms are in
[LICENSE](LICENSE). This file is the short, plain-English guide.

## Free use

You can use Nimbus for free if any of these apply:

- you are an individual developer
- you are a nonprofit organization
- you are an educational institution
- your organization has not exceeded both of these thresholds at the same time:
  - more than `$10M` USD annual revenue
  - more than `500` monthly active users

Free use includes:

- local development
- internal use
- self-hosting
- production use
- internal modifications (including maintaining private forks)

## When an enterprise license is required

An enterprise license is required only when your organization exceeds both:

- more than `$10M` USD annual revenue
- more than `500` monthly active users

If you exceed both thresholds, you receive a one-time `90-day` free trial. After
that, continued use requires a commercial enterprise license unless you later
drop below the threshold. The trial does not reset if you later exceed the
threshold again.

## What counts as a monthly active user

A monthly active user is a unique human user, internal or external, who in a
calendar month uses an application, service, or system backed by Nimbus.

That includes:

- employees using an internal Nimbus-backed tool
- customers using your Nimbus-backed product
- authenticated or otherwise identifiable end users who trigger Nimbus-backed
  activity

That does not normally include:

- purely automated service accounts
- background jobs
- bots that are not acting as a stand-in for a unique human user

If exact MAU counts are unavailable, you should estimate in good faith using
reasonable operational signals such as authenticated identities, tenant or user
IDs, request activity, or similar telemetry.

## What is always prohibited without a commercial license

Even if you otherwise qualify for free use or trial use, you may not without a
commercial license:

- offer Nimbus itself as a hosted or managed service
- provide a competing hosted backend, database, or developer platform built on
  Nimbus
- embed or white-label Nimbus as a material part of a competing platform
- use Nimbus branding as if your fork or service is official

Using Nimbus to power your own product is allowed. Offering Nimbus itself, or a
substantially similar managed platform, to third parties is not.

## Examples

- A startup with `$2M` annual revenue and `50,000` monthly active users may use
  Nimbus for free.
- A public company with `$500M` annual revenue and `100` monthly active users
  may use Nimbus for free.
- A company with `$25M` annual revenue and `2,000` monthly active users gets a
  `90-day` trial and then needs an enterprise license.
- A university may use Nimbus for free for teaching, research, or operations,
  as long as it is not offering a prohibited competing hosted service.
- A cloud vendor may not offer a managed Nimbus-like service without a
  commercial license, even if it would otherwise fit the revenue or MAU limits.

## Commercial licensing

Commercial licensing is for:

- organizations above the free-use threshold after the `90-day` trial
- hosted or managed Nimbus offerings
- OEM, embedded, or white-label platform deals
- enterprise support, security review, indemnity, or custom terms

See [COMMERCIAL.md](COMMERCIAL.md).

## Trademarks

Nimbus trademarks and branding are not open for unrestricted use. See
[TRADEMARKS.md](TRADEMARKS.md).
