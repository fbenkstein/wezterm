# Mux focus handling — findings (input for multiplexer redesign)

Status: investigation notes, 2026-06-28. All file:line refs are against the
current checkout and should be re-verified before relying on them.

## How this came up

Chasing a "fast tab switching causes flickering" report (assumed to be a
continuation of the macOS key-repeat bug fixed in `403756b69` /
`9a4f3c87c`). With a latency-injecting unix mux domain (75ms one-way proxy,
see `mux-redesign-notes`/`~/.config/wezterm/mux-delay-proxy.py`) the flicker
reproduces reliably. Instrumented logging proved it is **not** an input/key
problem at all — it is a focus-notification feedback loop in the mux client.

## Observed behavior (evidence)

Repro: press `Cmd+[` then `Cmd+]` in rapid succession on a tab backed by the
delayed domain.

- The macOS key path is clean: exactly **two** key events, both handled as
  `ActivateTabRelative`, then **zero** further key events. No stuck repeat.
  (The `perform_key_equivalent` fix is working.)
- After the two switches, the window enters a self-sustaining repaint storm:
  - idle baseline: ~**4** `Notification`/sec (≈250ms)
  - during the loop: ~**14** `Notification`/sec (≈**71ms**, i.e. ~the 75ms
    one-way proxy delay — the loop runs *at the speed of the link*)
  - the loop only stops on `FocusChanged(false)` (window loses focus), then
    returns to baseline. This is exactly the user's "switch to another window
    and back makes it stop" workaround.
- Doing the same two switches **slowly** (≥1s apart) never enters the loop —
  the burst settles back to 4/sec immediately. So it is a timing/ordering
  race that requires both switches to land inside one latency window.

## Root cause: focus echo loop

Tab switching is local, but it triggers a focus round-trip:

1. `activate_tab` → `tab.focus_changed(true)` (`wezterm-gui/src/termwindow/mod.rs:2201`)
   → `ClientPane::focus_changed` → `advise_focus()` sends a `SetFocusedPane`
   PDU to the server (`wezterm-client/src/pane/clientpane.rs:557,579-593`).
2. Server applies it and **broadcasts `PaneFocused` to *all* clients,
   including the originator** (`wezterm-mux-server-impl/src/sessionhandler.rs:331-364`,
   forwarded unconditionally at `wezterm-mux-server-impl/src/dispatch.rs:169`).
3. The client *applies* the echo: `Pdu::PaneFocused` →
   `mux.focus_pane_and_containing_tab` (`clientpane.rs:217-232`) →
   `Window::save_and_then_set_active` (`mux/src/lib.rs:554`) — i.e. it
   re-writes the local active tab, which re-arms `advise_focus` → another
   `SetFocusedPane`.

Under latency, two fast switches produce two **stale** echoes that arrive out
of phase with the user's intent. Each echo yanks the active tab and re-sends
focus, so the active tab ping-pongs A↔B at the round-trip rate = the flicker.

The dedup guard in `advise_focus` (`clientpane.rs:581`,
`if *focused_pane != Some(remote_pane_id)`) does not damp it, because focus
genuinely alternates B/A/B/A, so the value differs on every cycle.

## Architecture facts established (the useful part for redesign)

### Input routing is explicit and client-decided
Every input PDU names its target pane by `remote_pane_id`:
`SendKeyDown` (`clientpane.rs:464`), `WriteToPane` (`:380`),
`SendPaste` (`:346`), `SendMouseEvent` (`:417`), `Resize` (`:415`).
Input delivery therefore does **not** depend on server-side focus — focus is
a separate, auxiliary channel. (Note: `SendKeyDown` carries an `input_serial`
for predictive-echo reconciliation, not routing.)

Corollary: the *target* pane is chosen from the client's local active tab, so
while the loop bounces the active tab, freshly-typed input could be misrouted
even though each event is itself correctly addressed.

### "Active pane" is scoped, not global
- Per **tab**: exactly one (`Tab.active: usize`, `mux/src/tab.rs:45`;
  `get_active_pane` is `Option` only for teardown).
- Per **window**: at most one (active tab's active pane; `Window.active`,
  `mux/src/window.rs:12`).
- Per **client connection (domain)**: a single shared
  `focused_remote_pane_id: Mutex<Option<PaneId>>` (`wezterm-client/src/domain.rs:28`)
  — one value across *all* windows of that connection. Latent contention if
  two windows on one domain fight over focus.
- Per **server**: focus is tracked **per client** (`ClientInfo.focused_pane_id`,
  `mux/src/client.rs:54`; map at `mux/src/lib.rs:111`). With N clients there
  are up to N focused panes; there is **no single global active pane**.
- Wrinkles: overlays (copy-mode, tab navigator, debug) layer a pane the mux
  doesn't know about — hence `get_active_pane()` vs `get_active_pane_or_overlay()`;
  zoom doesn't change which pane is active.

### The server tracks focused *pane*, not "active tab"
There is no server concept of "the active tab" as an owned value the client
must obey. The server records a focused pane per client; the pane→tab mapping
and the act of switching the active tab are **client-side**
(`focus_pane_and_containing_tab`). (The server's own `Window.active` is updated
in the `SetFocusedPane` handler, but it is a single shared field per window —
itself a multi-client smell, separate from this bug.)

### Why the server needs focus at all (3 real reasons, all pane-level)
`LocalPane::focus_changed` → `Terminal::focus_changed`
(`term/src/terminalstate/mod.rs:767`):
1. **Focus-reporting escape sequences (DECSET 1004):** writes `CSI I`/`CSI O`
   to the PTY when the program enabled focus tracking (vim/neovim/tmux/fzf…).
   This is program-visible and *must* reach the server.
2. **Mouse-button release on blur:** synthesizes releases so apps don't get a
   stuck button.
3. **Unseen-output / activity tracking:** `lost_focus_seqno = seqno`, read by
   `has_unseen_output()` (`mux/src/localpane.rs:492`) for the tab activity dot.

### Purpose of the `PaneFocused` broadcast
It exists to propagate focus changes a client did **not** originate:
- another attached client (shared session),
- `wezterm cli activate-pane{,-direction}` / `activate-tab` hitting the server
  directly (the case the code comment calls out, `clientpane.rs:218-226`),
- internal server moves (focused pane closes → activate sibling,
  `mux/src/tab.rs:1782`; tmux integration `mux/src/tmux_commands.rs:345`).

For changes the client made itself, the echo is pure redundancy. The flaw is
that it is **echoed back to the originator with no origin tag**, so the
originator cannot distinguish "my own change" from "someone else's."

### Identities already exist on both ends (but aren't used here)
- The **client mints its own `ClientId`** (`ClientId::new()`,
  `wezterm-client/src/client.rs:1046`), retains it (`Client.client_id`, `:58`),
  and registers it via `SetClientId` at connect (`:1166`).
  `ClientId = {hostname, username, pid, epoch, id, ssh_auth_sock}`
  (`mux/src/client.rs:17`).
- The **server keys all per-client state by `ClientId`**
  (`HashMap<ClientId, ClientInfo>`, `mux/src/lib.rs:111`), binds the active
  identity during request handling (`with_identity`, `lib.rs:673`), and records
  focus per-identity (`record_focus_for_current_identity`, `lib.rs:499`).

So both sides know who the originator is. The gap is purely that the
**notification path is identity-blind**: `MuxNotification::PaneFocused(pane_id)`
and the `PaneFocused { pane_id }` PDU (`codec/src/lib.rs:823`) carry no origin,
and `dispatch.rs:169` fans out to every client without identity scoping. More
generally, outbound notifications are not scoped by identity even though the
server has it.

## Design implications for the redesign

1. **Make notifications identity-aware.** Server-originated notifications
   should be able to carry/elide the originating `ClientId`, so a client can
   ignore (or never receive) echoes of its own actions. This is the clean,
   general fix and removes a whole class of "my own change comes back and
   fights me" races, not just focus.
2. **Separate "intent" from "confirmation."** A client's local focus/active-tab
   is authoritative for that client and should not be silently overwritten by a
   confirmation of a change it just made. Treat inbound focus as authoritative
   only when exogenous.
3. **Reconsider single shared fields.** `client.focused_remote_pane_id` (one
   per connection, across windows) and the server's per-window `Window.active`
   (one, across clients) both flatten multi-window/multi-client focus into a
   single slot. The redesign should make focus/active-selection
   per-(client,window) where appropriate.
4. **Focus vs input are already decoupled — keep it that way.** Input is
   explicitly addressed; focus is auxiliary (DECSET 1004 / activity / multi
   client). The redesign should preserve this and not let focus state become
   load-bearing for routing.

## Fix options for the current bug (pre-redesign)

- **Server-side (preferred):** in the `SetFocusedPane` handler, don't deliver
  the resulting `PaneFocused` back to the originating client (identity is
  already bound there), or add the origin `client_id` to the PDU so clients
  skip their own. Fixes all clients at once, no heuristics.
- **Client-side (defensive backup):** when applying a *remote* `PaneFocused`,
  do not let it re-trigger `advise_focus` — break the apply→re-send arm so an
  inbound focus can never bounce back out.

## Still to confirm
- Capture the PDU-level ping-pong directly:
  `WEZTERM_LOG=info,wezterm_client=trace` and look for alternating
  `set_focused_pane` sends and `advised of remote pane focus:` lines
  (`clientpane.rs:227`) during the fast repro.
