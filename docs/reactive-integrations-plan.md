# Reactive Integrations - planning doc

## Status: Teams integration implemented (2026-09-09)

The Teams half of this is now built - see `src/teams.rs`, `store::ReactiveSettings`,
and the "Reactive Integrations" section of the Settings tab. It ended up using
option 4 below (teams-for-linux's own MQTT publisher), which wasn't in the
original options list - no Graph API/OAuth needed after all.

**Confirmed status values** (from a real teams-for-linux + Mosquitto setup):
`available`, `busy`, `do_not_disturb`, `away` (statusCodes 1-4 respectively).

**Known limitation**: "Appear Offline" and "Be Right Back" were both tested
live and teams-for-linux reports **both as `away`** - its own status
detection doesn't distinguish them. This is a limitation of the upstream
teams-for-linux MQTT feature, not something fixable in this app; if it adds
finer-grained detection later, more rows can be added to the mapping UI.

Goal: automatically send an IR signal (e.g. change RGB light color) in
reaction to the user's status in a chat app, instead of requiring a manual
button click every time. First target: **Microsoft Teams**. **Discord** is
a noted future goal but out of scope for now.

## Desired behavior

- Watch the local Teams client's presence status (Available / Busy / In a
  meeting / Do not disturb / Away / etc).
- Map each status to a saved button (e.g. a color) in this app's existing
  remote/button library.
- When status changes, automatically fire the corresponding `Send`, the same
  way a manual button click does today.
- Should be toggleable (on/off) and configurable (which button maps to which
  status), not force-on.

## How to read Teams status - options

Microsoft Teams has no simple, stable, officially-supported local API for
reading presence from a third-party desktop app. Options, roughly in order
of robustness vs. effort:

1. **Microsoft Graph API `presence` endpoint** (`GET
   /me/presence` or `/users/{id}/presence`). Officially supported and
   stable, but requires:
   - Registering an Azure AD app and going through OAuth (delegated
     permissions: `Presence.Read`).
   - The user's org allowing this (some tenants restrict Graph API app
     registrations).
   - Handling token refresh, so this app would need a small OAuth flow
     (device code flow is probably the least painful for a desktop app).
   - This is the "do it properly" option and the one most likely to keep
     working across Teams client updates, but has real setup overhead per
     user (they'd need to consent to an app registration, or we'd need one
     shared multi-tenant app registration they sign into).

2. **New Teams client's local presence API** (undocumented). The new Teams
   client (as of 2024+) exposes a local presence/notification API over a
   loopback websocket for third-party integrations (this is what things like
   Stream Deck Teams plugins and USB busy-light integrations use). Needs
   more research: whether it requires a similar app-registration/token
   step, what the actual local endpoint/protocol is, and whether it's
   available on Linux (the user is on Linux; Teams' Linux support situation
   should be checked first - the classic Teams Linux client was
   discontinued, so this may only be reachable if Teams is used via a web
   browser tab, PWA, or a container/VM running Windows).

3. **Screen-scraping / log-watching** (fragile, not recommended as a
   primary approach, but worth noting): some hobby projects watch Teams'
   local log files or window title for status hints. Breaks on any Teams
   UI/version change and is the least maintainable option.

Given the user is on Linux, **step 0 of this plan should be confirming how
they actually run Teams** (browser tab, PWA, or a VM/other machine) before
picking an approach - that determines which of the above is even reachable.

## Discord (future work, not started)

Discord has an official local IPC mechanism (Discord RPC / "Rich Presence")
that third-party apps can connect to locally without OAuth, which is
generally easier to integrate with than Teams. Revisit once the Teams
integration pattern (status -> mapped button -> auto-send) is working, since
most of the plumbing (a background "reactive integrations" watcher thread,
a status-to-button mapping UI, an enable/disable toggle) should be reusable
across both.

## Rough shape of the eventual feature (not built yet)

- A new Settings sub-section: "Reactive Integrations", with a per-service
  (Teams, Discord) enable toggle and a status -> button mapping table.
- A background watcher thread per enabled integration, analogous to the
  existing `device_worker`/update-check pattern in this app - polls or
  subscribes to status changes and pushes a `Send` request through the
  existing device request channel when the mapped status changes.
- Needs a mapping UI: for each known status value, let the user pick which
  saved remote+button to fire (reusing the existing remote/button data,
  no new storage format needed beyond the mapping itself).
