# Security policy

taktora is a **pre-1.0 personal experiment**. The threat model
and operational guarantees match the README's status statement:
no SLA, no published crates.io release, no production-readiness
claim, and the `unsafe` story has not been independently audited.

This document explains how to report a vulnerability anyway.

## Supported versions

| Version | Supported |
|---------|-----------|
| latest commit on `main` | ✅ best-effort |
| anything else (tags, forks, vendored copies) | ❌ no |

There is no released stable version. Fixes land on `main` and
nowhere else.

## Reporting a vulnerability

**Please do not file a public GitHub issue for security reports.**

Use GitHub's private security-advisory flow:

→ <https://github.com/patdhlk/taktora/security/advisories/new>

Include:

- A description of the issue and its impact.
- The affected crate(s) and, if known, the commit SHA where the
  issue first appeared.
- A minimal reproducer if you have one.
- Your preferred coordination handle (GitHub username or email).

I'll acknowledge the report when I see it. Fix and disclosure
timing are best-effort and depend on severity and my availability.

## Response expectations

- **Acknowledgement:** best-effort, no SLA.
- **Fix:** lands on `main`; no backport, no patch-release pipeline.
- **Disclosure:** coordinated with the reporter via the advisory
  thread. Once a fix is merged on `main`, the advisory is published.
- **Bounty:** none.

## Out of scope

Anything that depends on shipping taktora to production. The README
is explicit: this is a personal experiment, not a maintained library.
If a vulnerability only matters because you've taken on the risk of
shipping pre-1.0 unaudited code, that's a deployment decision and a
private advisory still helps — but please calibrate expectations
about response time accordingly.
