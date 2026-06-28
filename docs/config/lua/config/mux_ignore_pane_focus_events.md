---
tags:
  - multiplexing
---
# `mux_ignore_pane_focus_events`

{{since('nightly')}}

When `true`, a multiplexer client ignores incoming `PaneFocused`
notifications from the mux server.

When focus moves between panes, the server records the focused pane and
broadcasts a `PaneFocused` notification to *every* connected client,
including the one that initiated the change. Normally a client applies that
notification by activating the corresponding tab/pane locally. This is what
allows focus changes made by another client (or by `wezterm cli
activate-pane`, `activate-pane-direction`, `activate-tab`) to be reflected
everywhere.

However, because the originating client also receives the echo of its own
focus change, under connection latency a rapid sequence of tab switches can
set up a feedback loop: the stale, out-of-order echoes keep re-activating
tabs, producing a flicker that only stops when the window loses focus.

Setting this to `true` breaks that loop by making the client treat
`PaneFocused` as purely informational and not act on it. The local effect of
your own focus changes is unaffected, and the server is still told about
focus (via `SetFocusedPane`), so focus-reporting escape sequences and
unseen-output / activity tracking continue to work. The trade-off is that
focus changes driven by *other* clients or by `wezterm cli activate-*` will
no longer be reflected in this client.

The default is `false`.

```lua
config.mux_ignore_pane_focus_events = true
```
