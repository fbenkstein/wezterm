# Mux Server Spawn Improvements

Two independent improvements to how the GUI spawns `wezterm-mux-server`.

## Problem 1: fork() on macOS

The GUI currently spawns the mux server via Rust's `std::process::Command` with
a `pre_exec` hook that restores the user's original umask in the child after
`fork()`. This forces the fork+exec path even on macOS, where `fork()` in a
process that has touched Cocoa/Mach ports is unsafe.

Rust's `Command` uses `posix_spawn` when no `pre_exec` hooks are registered.

**Fix:** Pass the umask as a CLI argument, e.g. `--umask 0022`. The server calls
`umask(value)` early in `main()` itself. No `pre_exec` needed, Rust uses
`posix_spawn`.

Note: the same umask issue applies to PTY children (shells, user programs), but
those are arbitrary binaries that can't be modified, so `pre_exec` is
unavoidable there.

## Problem 2: socket connection race

After spawning the server, the client polls the socket path with retries
(`unix_connect_with_retry`) waiting for the server to bind and listen. This is
inherently racy and adds startup latency.

**Fix:** Socket activation — the GUI creates and binds the socket before
spawning the server:

1. GUI calls `bind()` + `listen()` on the socket path.
2. GUI spawns the server with the listening fd inherited, e.g. `--socket-fd 5`.
3. GUI connects immediately — the kernel already has a listen backlog, so
   `connect()` succeeds before the server calls `accept()`.
4. Server skips its own socket setup and calls `accept()` on the inherited fd.

This is the same pattern as systemd/launchd socket activation. No polling loop,
no timing dependency.

## Combined effect

With both fixes, spawning the mux server on macOS becomes a clean `posix_spawn`
call with no `pre_exec` hooks and no post-spawn retry loop.
