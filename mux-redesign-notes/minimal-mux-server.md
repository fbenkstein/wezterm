# Minimal Mux Server Design

## Core thesis

The current mux server reads the full WezTerm config and implements multiple
domain types, but nearly all of that complexity belongs on the client. A
minimal server needs to be only three things: a Unix socket listener, a PTY
spawner, and a VT parser.

## What the server currently reads from config

The full set, grouped by category:

**Transport / listeners**
- `unix_domains` — socket path(s) to listen on
- `tls_servers` — TLS listener config
- `tls_clients`, `ssh_domains` (WezTerm multiplexing), `wsl_domains`,
  `exec_domains`, `serial_ports` — domains registered into the mux

**Process / environment**
- `update_ulimit` — raise fd limits on startup
- `default_ssh_auth_sock` — sets `SSH_AUTH_SOCK`
- `mux_env_remove` — env vars to strip before spawning
- `daemon_options` — log file paths when daemonized

**Initial shell**
- `initial_size`, `default_prog`, `default_cwd`, `set_environment_variables`

**VT parser / terminal emulator** (via `TermConfig`)
- `scrollback_lines`, `enable_kitty_graphics`, `enable_kitty_keyboard`,
  `enable_csi_u_key_encoding`, `enable_title_reporting`,
  `enable_checksum_rectangular_area`, `log_unknown_escape_sequences`,
  `canonicalize_pasted_newlines`, `normalize_output_to_unicode_nfc`,
  `alternate_buffer_wheel_scroll_speed`, `unicode_version`, `bidi_enabled`,
  `bidi_direction`, `enq_answerback`, `debug_key_events`,
  `resolved_palette` (pushed to clients as `SetPalette` on config reload)

**Output parser tuning**
- `mux_output_parser_buffer_size`, `mux_output_parser_coalesce_delay_ms`

**SSH agent / backend**
- `mux_enable_ssh_agent`, `ssh_backend`

**Lua / events**
- Fires `mux-startup` event, so anything in that Lua handler also runs

## What to drop

### SSH in the server (`RemoteSshDomain`)

`ssh_domains` with `multiplexing = "None"` cause the server to open SSH
connections itself via `RemoteSshDomain`. This is the only case where the
server does networking beyond its own listener. Drop it.

The replacement is the jump-host model that already exists for
`SshMultiplexing::WezTerm`: the client SSHes to the remote machine and runs
`wezterm-mux-server proxy` over that connection, bridging stdin/stdout to the
Unix socket. The server never touches SSH; it just sees another local client.

Code that drops with this:
- `mux/src/ssh.rs` — `RemoteSshDomain` (~800 lines)
- `wezterm-mux-server-impl/src/pki.rs` — TLS PKI for mTLS bootstrap
- `wezterm-mux-server/src/ossl.rs` — TLS listener
- `GetTlsCreds` / `GetTlsCredsResponse` PDUs
- `SshMultiplexing::None` variant

### Multiple domain types

The server currently supports `LocalDomain`, `WslDomain`, `ExecDomain`,
`SerialDomain`, and (via client domains) remote mux connections. All of the
local ones collapse to the same thing: **fork a process and connect it to a
PTY**.

- **`WslDomain`**: rewrites `bash` → `wsl.exe -d Ubuntu -- bash`. This
  rewriting can happen on the client before `SpawnV2` is sent. Windows-only.
- **`ExecDomain`**: calls a Lua `fixup_command` callback to rewrite the
  `SpawnCommand`. Lua already runs on the client; the callback should run
  there and the server receives the final resolved command.
- **`SerialDomain`**: opens `/dev/ttyUSB0` directly as a PTY-like fd. The
  "child process" is a fiction — no PID, `kill()` is a NOP, `wait()` polls
  carrier-detect in a loop, `resize()` is a NOP. Functionally equivalent to
  running `picocom -b 115200 /dev/ttyUSB0` as a command. The one thing lost
  is native carrier-detect exit detection, but terminal serial tools handle
  device removal via their own exit path anyway.

**Conclusion**: the server's domain model collapses to a single local PTY
spawner. The multi-domain concept becomes purely a client-side organizational
tool — a named grouping that applies a transformation to spawn requests before
sending `SpawnV2`. No server-side representation needed.

### Lua runtime

Without `mux-startup`, `ExecDomain.fixup_command`, and config-driven domain
registration, the server has no use for a Lua runtime. Dropping it removes
the largest startup cost and eliminates a whole class of server-side bugs.

## What stays, simplified

### Socket path

Can be derived from a convention (e.g., `$XDG_RUNTIME_DIR/wezterm-mux.sock`
or `~/.local/share/wezterm/mux-$UID.sock`) or passed as a CLI argument. No
config file needed for this.

### VT parser settings

These affect stored cell state and must be decided at server startup. Two
options:

**Option A — CLI flags**: Server accepts `--scrollback-lines N`,
`--enable-kitty-graphics`, etc. The client's local config determines which
flags to pass when launching the server. No config file on the server.

**Option B — Per-spawn negotiation**: Extend `SpawnV2` to carry terminal
configuration parameters. Server uses what the spawning client sends. Cleaner
for multi-client heterogeneity but adds protocol complexity and raises
questions about disagreements between clients.

Option A is simpler and matches how tmux handles it (fixed server config,
clients don't negotiate VT settings).

### Environment cleanup

`mux_env_remove` has sensible fixed defaults (`OLDPWD`, `PWD`, `SHLVL`,
`WEZTERM_PANE`, `WEZTERM_UNIX_SOCKET`, `_`). Additional entries could be a
CLI flag rather than config.

### Output parser tuning

`mux_output_parser_buffer_size` and `mux_output_parser_coalesce_delay_ms` are
performance knobs with reasonable compiled-in defaults. CLI flags if needed.

## Resulting minimal server interface

```
wezterm-mux-server [OPTIONS] [-- PROG...]

Options:
  --socket <PATH>              Unix socket path [default: derived from XDG]
  --daemonize                  Detach and run in background
  --scrollback-lines <N>       [default: 3500]
  --enable-kitty-graphics      Enable kitty graphics protocol
  --enable-kitty-keyboard      Enable kitty keyboard protocol
  [other VT parser flags...]
  --cwd <DIR>                  Working directory for initial pane
  PROG                         Command to run (default: $SHELL)

Subcommands:
  proxy [--expect-sha <HEX>]   Bridge stdin/stdout to the mux socket
```

No `--config-file`. No Lua. The client's config drives the flags passed at
launch time.

## What moves to the client

- Domain transformation (WSL wrapping, exec domain fixup, serial-as-command)
- SSH connection and proxy tunnel setup
- TLS if needed (the client already manages TLS for the `SshMultiplexing::WezTerm` path)
- `mux-startup` equivalent: client requests initial spawn(s) after connecting
- Config reload effects: no more `SetPalette` push from the server on config
  change (server config is static after startup)
