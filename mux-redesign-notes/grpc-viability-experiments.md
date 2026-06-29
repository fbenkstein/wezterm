# gRPC Viability Experiments

## Status

Draft experiment plan for deciding whether gRPC over HTTP/2 is viable for the
new streaming mux implementation.

This document is intentionally split into independent sections. Each experiment
should be executable on its own and should leave behind a short result note,
prototype branch, benchmark output, or failure report.

## Results so far (2026-06-29)

Driven via two sub-agents — a fit-for-wezterm integration study (read-only,
against the wezterm tree) and a standalone tonic/prost prototype that ran
experiments 1–4 for real. **Verdict: gRPC is viable; proceed.** No blocker.

- Gating transport experiments **pass empirically**: Unix socket (exp 1) and a
  raw `AsyncRead+AsyncWrite` byte-stream stand-in for SSH stdio (exp 2) both
  work; small-message RTT is ~16 µs (~12 µs over a raw socket), and HTTP/2
  per-stream flow control isolates a slow reader (exps 3–4).
- Integration is **viable-with-work, no blocker.** The one real cost is the
  runtime split: wezterm runs no tokio (it uses smol + a main-thread `promise`
  executor — see `promise/src/spawn.rs:40`), so the gRPC domain must run its own
  tokio runtime on a dedicated thread and bridge to the main-thread `Mux` over
  channels. The deps (`tokio`/`hyper`/`h2`/`tower`/`rustls`) are already in
  `Cargo.lock`; `tonic`/`prost` add little and conflict with nothing.
- **Decision:** use vendored protoc or pure-Rust `protox` for codegen — do not
  add a system `protobuf-compiler` dependency (CI/`get-deps` have none today).
- **Remaining gate:** Experiment 9 (in-process runtime integration) — the
  tokio↔main-thread-`promise` bridge is the load-bearing risk a standalone
  prototype can't prove. In progress.

## Decision Target

The current protocol draft chooses gRPC over HTTP/2 with protobuf IDL for the
new mux implementation. That remains the preferred direction because it gives
WezTerm a standard protocol surface, generated client/server code, explicit IDL,
and a path for alternative implementations.

These experiments are meant to find concrete blockers early. A custom protobuf
envelope should only be reconsidered if gRPC fails one of the required transport
or performance cases in a way that cannot be reasonably worked around.

## Shared Success Criteria

gRPC is viable if:

- It works over Unix domain sockets.
- It can support the SSH proxy/deployment shape, either directly or through a
  small well-contained bridge.
- Bidirectional and server-streaming RPCs behave predictably under terminal-like
  traffic.
- Flow control can prevent a slow client or pane stream from blocking unrelated
  panes, input, or control messages.
- Snapshot and blob payloads can be transported without awkward limits or
  excessive copying.
- Reconnect and failure behavior is understandable enough for long-running
  sessions.
- Standard tooling can inspect or exercise the service enough to justify the
  protocol choice.

## Common Prototype

### Question

What is the smallest useful gRPC service skeleton for the experiments?

### Setup

Create a throwaway Rust prototype, ideally outside the production mux code at
first. Use `tonic` and `prost`.

The IDL can be a reduced version of `streaming-mux-protobuf-protocol.md`:

```proto
service StreamingMux {
  rpc Control(stream ClientControl) returns (stream ServerControl);
  rpc AttachPane(AttachPaneRequest) returns (PaneSnapshot);
  rpc ReadPane(ReadPaneRequest) returns (stream PaneReadEvent);
  rpc WritePane(stream PaneInput) returns (WritePaneResponse);
  rpc ResizePane(ResizePaneRequest) returns (ResizePaneResponse);
}
```

Messages can be minimal. The prototype does not need a real PTY initially; a
scripted output generator is enough for most transport and flow-control tests.

### Deliverable

- Prototype source.
- Exact crate versions.
- Basic run instructions.

## Experiment 1: Unix Domain Socket Transport

### Question

Can the selected Rust gRPC stack serve and connect over Unix domain sockets in
a way that fits the local mux-server launch path?

### Why It Matters

The local mux domain should not require TCP. Unix sockets are the natural local
transport and preserve the current security/deployment shape.

### Procedure

1. Serve the prototype using a Unix listener and `serve_with_incoming`.
2. Connect the client using a custom connector such as Tonic's
   `Endpoint::connect_with_connector`.
3. Run all prototype RPCs over the Unix socket.
4. Verify cleanup behavior when the socket path already exists, the server
   exits, and the client reconnects.

### Pass Criteria

- All RPC shapes work over a Unix socket.
- The code required to adapt Tonic to Unix sockets is small and local.
- Errors are diagnosable enough for user-facing connection failures.

### Failure Criteria

- Unix socket support requires invasive changes or unstable/private APIs.
- Reconnect or cleanup behavior is unreliable enough to threaten mux startup.

### Deliverable

- Short result note with code snippets for server and client setup.

### Result

Date: 2026-06-29
Branch/prototype: standalone `/tmp/grpc-spike` (tonic 0.12.3, prost 0.13, tokio full)
Verdict: pass

Findings:
- All four RPC shapes (unary, server-stream, client-stream, bidi) run over a
  Unix socket. Server: `serve_with_incoming(UnixListenerStream::new(uds))` —
  `UnixStream` already implements tonic's `Connected`, so no wrapper is needed.
  Client: `Endpoint::connect_with_connector(service_fn(|_| UnixStream::connect))`
  with a dummy URI; the client wraps the stream in `hyper_util::rt::TokioIo`, the
  server must not (tonic wraps internally).
- Glue is minimal (a few lines each side).

Follow-up:
- For production, peer-credential auth (`SO_PEERCRED`/`getpeereid`) is available
  off the raw `UnixStream` but needs a custom `Connected` impl to surface via
  tonic `ConnectInfo`. Reuse `safely_create_sock_path` for socket security.

## Experiment 2: SSH Proxy Transport

### Question

Can gRPC be carried through the mux's remote deployment shape?

### Why It Matters

The redesigned mux still needs to support remote sessions. The current mux can
ride over an SSH stdio tunnel. gRPC assumes HTTP/2 over an ordered byte stream,
so the experiment is whether that assumption can be satisfied cleanly.

### Procedure

Test at least one of these approaches:

- Run a gRPC server on the remote side listening on a Unix socket, then forward
  it with OpenSSH local forwarding.
- Implement a small `wezterm-mux-server proxy-grpc` command that bridges local
  client traffic to the remote Unix socket.
- If practical, adapt stdin/stdout into an `AsyncRead + AsyncWrite` transport
  and serve/connect Tonic over that stream.

Run `Control`, `ReadPane`, and `WritePane` through the selected path.

### Pass Criteria

- Remote attach, output streaming, input streaming, and reconnect work.
- The proxy code is understandable and isolated.
- The approach does not require exposing remote TCP listeners by default.

### Failure Criteria

- SSH proxying requires a fragile HTTP/2-aware tunnel.
- The implementation cannot preserve the simple "run server over SSH" model.
- Error handling becomes substantially worse than the current mux tunnel.

### Deliverable

- Recommended remote transport shape.
- Notes on whether stdio transport is viable or whether SSH forwarding/proxying
  is preferred.

### Result

Date: 2026-06-29
Branch/prototype: standalone `/tmp/grpc-spike`; integration study against the wezterm tree
Verdict: pass (custom-stream proven; real `ssh --stdio` retest outstanding)

Findings:
- **The load-bearing transport result.** tonic served and connected over a bare
  `tokio::io::duplex()` byte pipe — no socket, no listener, no addressing — for
  all four RPC shapes. tonic speaks HTTP/2 prior-knowledge (h2c) straight onto
  the stream, so any ordered bidirectional byte stream works. The only glue is
  one `Connected` newtype on the server delegating `AsyncRead`/`AsyncWrite`, fed
  via a one-element `serve_with_incoming` stream.
- Maps directly to SSH: substitute `tokio::process::ChildStdin/ChildStdout` for
  the duplex halves. wezterm's existing SSH path is already a byte-clean netcat
  bridge (`wezterm/src/cli/proxy.rs`) piping stdio ↔ a remote Unix socket;
  HTTP/2 tunnels through it transparently. Recommended: keep the bridge, re-point
  at the gRPC Unix socket (or adapt `SshStream`, `wezterm-client/src/client.rs:545`,
  to tokio I/O for a direct stdio transport). No remote TCP exposure needed.

Follow-up:
- Re-run over a real `ssh host wezterm-mux-server proxy` using
  `ChildStdin`/`ChildStdout`; confirm SSH pipe close surfaces as a graceful
  HTTP/2 stream end, not a transport panic.

## Experiment 3: Streaming Latency and Throughput

### Question

Does gRPC streaming introduce acceptable overhead for terminal output?

### Why It Matters

Mux traffic includes many small writes, occasional large bursts, and long idle
periods. The protocol must not add visible latency or excessive CPU overhead.

### Procedure

Replay synthetic workloads through `ReadPane`:

- 1-byte, 8-byte, 64-byte, 1 KiB, and 16 KiB chunks.
- Bursty command output.
- Continuous high-throughput output.
- Long idle periods with occasional output.
- Many panes with low-rate output.

Measure:

- end-to-end latency from server send to client receive,
- throughput,
- CPU,
- allocation rate if practical,
- memory growth over time.

Compare against:

- current mux codec if convenient,
- a minimal custom length-prefixed protobuf stream, if cheap to implement.

### Pass Criteria

- Small-message latency is not visibly worse than the current mux path.
- High-throughput output does not consume unreasonable CPU.
- Long idle streams do not leak memory or tasks.

### Failure Criteria

- Per-message overhead forces unnatural batching that would hurt interactivity.
- CPU or allocation overhead is clearly out of proportion to the current mux.

### Deliverable

- Benchmark table.
- Recommended output chunking/coalescing policy.

### Result

Date: 2026-06-29
Branch/prototype: standalone `/tmp/grpc-spike` (loopback; treat numbers as a floor)
Verdict: pass

Findings:
- Interactive round-trip (the keystroke-echo metric), sequential ping→ack: gRPC
  bidi RTT ~16.5 µs p50 / ~31 µs p99 vs ~5 µs for a raw 1-byte ping-pong — gRPC
  adds ~12 µs per round-trip, ~3 orders of magnitude under the ~20–30 ms human
  threshold and lost in network RTT on any real link.
- Streaming throughput: >2M small msgs/s; ~150 MiB/s at 64 B, ~1 GiB/s at 16 KiB.
  Raw framing edges gRPC only at 16 KiB (~1.2 vs ~1.0 GiB/s); gRPC beats the
  naive raw baseline at 1–64 B due to internal batching. Large per-message
  "latency" at big chunk sizes is burst-queueing, not per-message cost.

Follow-up:
- Re-measure RTT over a real SSH tunnel (expected to stay in the network-RTT
  noise).

## Experiment 4: Flow Control and Slow Clients

### Question

Can HTTP/2 flow control keep a slow pane/client from blocking unrelated work?

### Why It Matters

The mux server must continue reading PTY output and serving other panes even if
one client stops reading one stream. Long-running sessions cannot let a stalled
viewer back up the whole server.

### Procedure

Create:

- one fast `ReadPane` stream,
- one deliberately slow or paused `ReadPane` stream,
- an active `WritePane` stream,
- an active `Control` stream.

Run heavy output through the slow pane while sending input/control traffic to
other panes.

Vary:

- HTTP/2 stream window size,
- HTTP/2 connection window size,
- adaptive window settings,
- bounded server-side output queue sizes.

### Pass Criteria

- Slow readers apply backpressure only to their own bounded queues.
- Other panes and control messages remain responsive.
- The server has a clear policy for dropping, disconnecting, or resyncing a
  client whose output queue exceeds limits.

### Failure Criteria

- One stalled stream blocks the connection or service broadly.
- Flow-control behavior is too opaque to reason about safely.

### Deliverable

- Recommended queue/backpressure policy.
- HTTP/2 tuning values or tuning strategy.

### Result

Date: 2026-06-29
Branch/prototype: standalone `/tmp/grpc-spike`
Verdict: pass

Findings:
- One fast `ReadPane`, one slow `ReadPane` (reads 5 messages then sleeps), and an
  active bidi `Control` on a single HTTP/2 connection, each with a bounded
  server-side `mpsc` (cap 32). The slow stream stalled at 5; the fast stream
  drained 2000/2000 and the control stream stayed fully responsive. HTTP/2
  WINDOW_UPDATE backpressure held the slow producer on its own stream only — no
  head-of-line blocking of other panes or control.
- Consequence: **HTTP/2 per-stream flow control subsumes the manual ack +
  water-mark backpressure scheme** from the replicated-terminal design; the
  server just needs a bounded per-stream queue.

Follow-up:
- Decide the queue-overflow policy: block-producer (tested) vs. drop-and-resync
  the pane vs. coalesce-to-latest-screen. A protocol-design choice, not a
  transport limitation.

## Experiment 5: Snapshot and Blob Payloads

### Question

Can gRPC carry pane snapshots and image/blob payloads cleanly?

### Why It Matters

Attach and reconnect depend on snapshots. Terminal graphics may require large
binary payloads. Default gRPC message-size limits may be too small or awkward.

### Procedure

Generate snapshots at several sizes:

- normal viewport with modest scrollback,
- large scrollback tail,
- many styled cells,
- hyperlinks,
- image references,
- large image blobs.

Test:

- single-message snapshots,
- chunked snapshot streams,
- separate `GetBlob` unary calls,
- separate chunked blob streams.

### Pass Criteria

- Reasonable snapshots fit without surprising limits.
- Large blobs have a clean chunking path.
- Client memory usage is bounded during attach.

### Failure Criteria

- Required payloads force excessive max-message sizes.
- Chunking makes the API substantially more complex than expected.

### Deliverable

- Recommended snapshot and blob transfer policy.
- Initial max-message and chunk-size settings.

## Experiment 6: Reconnect and Failure Behavior

### Question

Does gRPC behave well when clients disappear and replacement clients attach?

### Why It Matters

The most important multi-client-like case is a replacement client after network
timeout, laptop sleep, or client reboot.

### Procedure

Run a session, then test:

- killing the client process,
- closing only `ReadPane`,
- closing only `Control`,
- severing SSH transport,
- pausing the client for longer than keepalive thresholds,
- reconnecting while the server still believes the old client is alive.

Observe:

- server detection time,
- resource cleanup,
- stream cancellation behavior,
- replacement attach behavior,
- unsolicited `PaneSizeChanged` behavior.

### Pass Criteria

- The server eventually notices dead clients.
- Replacement clients can attach from snapshots without waiting for perfect old
  client cleanup.
- Stale streams do not retain unbounded output queues.

### Failure Criteria

- Dead client detection is too slow without unacceptable keepalive settings.
- Partial stream failures leave the session in a confusing state.

### Deliverable

- Recommended keepalive/deadline/cancellation policy.
- Replacement-client behavior notes.

## Experiment 7: Cross-Stream Race Semantics

### Question

Can the implementation tolerate races between input, output, resize, and
control streams without relying on a false total order?

### Why It Matters

PTY semantics do not provide deterministic ordering between input bytes, output
bytes, resize ioctls, signals, and application repaint behavior. The protocol
must match that reality.

### Procedure

Create tests that race:

- `ResizePane` against high-rate `ReadPane` output,
- `WritePane` input against `ResizePane`,
- unsolicited `PaneSizeChanged` against local resize speculation,
- `ActivatePaneCommand` against local active-pane changes.

Use assertions for protocol invariants, not exact terminal viewport equality.

### Pass Criteria

- No code path assumes cross-stream ordering.
- Clients accept unsolicited size changes.
- Clients can request corrective non-increasing resizes.
- Divergence triggers hash mismatch or explicit resync rather than deadlock.

### Failure Criteria

- Correctness depends on timing between separate streams.
- Resize races produce persistent unrecoverable divergence.

### Deliverable

- Race test cases.
- Any required protocol clarifications.

## Experiment 8: Tooling and Alternative Clients

### Question

Does gRPC provide enough practical tooling benefit to justify choosing it over a
custom envelope?

### Why It Matters

The main reason to prefer gRPC is not serialization. It is a standard protocol
surface with existing tools and a lower barrier for alternative implementations.

### Procedure

1. Enable server reflection if feasible.
2. Use `grpcurl` or equivalent tooling to call unary methods.
3. Generate a small non-Rust client, such as Go or Python.
4. Have the alternative client attach to a pane, read output, and send input.
5. Inspect traces/logs with standard gRPC or HTTP/2 tooling.

### Pass Criteria

- Basic inspection and unary calls work with off-the-shelf tools.
- A simple alternative client can be built without understanding WezTerm
  internals.
- The tooling helps debug real protocol issues.

### Failure Criteria

- The service shape is too streaming-heavy for standard tools to help.
- Alternative clients still require too much WezTerm-specific private knowledge.

### Deliverable

- Minimal alternative client.
- Tooling notes and commands.

## Experiment 9: WezTerm Runtime Integration

### Question

Can Tonic integrate cleanly with WezTerm's runtime, threading, and shutdown
model?

### Why It Matters

A standalone prototype can succeed while integration into the real GUI/server
process remains awkward.

### Procedure

Create a thin integration spike that:

- starts a gRPC mux server from the GUI launch path,
- connects from the client side,
- cleanly shuts down on process exit,
- logs through WezTerm's logging infrastructure,
- coexists with existing async/runtime components.

This does not need to implement the full mux protocol.

### Pass Criteria

- Startup/shutdown are deterministic.
- Runtime ownership is clear.
- The integration does not require broad unrelated refactors.

### Failure Criteria

- Tonic introduces runtime conflicts or shutdown hazards.
- The integration is too invasive for an experimental side-by-side domain.

### Deliverable

- Integration notes.
- List of required production-code touch points.

### Result

Date: 2026-06-29
Branch/prototype: integration study (read-only) complete; in-process spike in progress
Verdict: inconclusive — viable-with-work per analysis; empirical spike underway

Findings:
- wezterm runs **no tokio** in its shipping processes; it uses smol
  (`async-io`/`async-executor`/`async-task`) plus a main-thread `promise`
  executor (`promise/src/spawn.rs:40-44` documents why: a GUI app's main-thread
  loop can't host a tokio/mio reactor). tonic hard-requires tokio. tokio is in
  the lock only via `sync-color-schemes`/`reqwest`, not linked into
  wezterm/gui/mux-server.
- So the gRPC domain must run a **dedicated tokio runtime on its own thread** and
  bridge to the main-thread `Mux` over channels — matching the existing
  `smol::channel` fan-in in `wezterm-mux-server-impl/src/dispatch.rs`. `Mux` work
  is `!Send`/main-thread (`AsyncReadAndWrite` is `async_trait(?Send)`,
  `wezterm-client/src/client.rs:519`), so the bridge must keep mux-touching work
  on the main thread.
- Contained to the experimental domain; no executor refactor. This is the
  load-bearing risk and the reason for the dedicated in-process spike.

Production-code touch points (from the integration study):
1. Dedicated tokio runtime host on its own thread, bridged to `Mux` via channels.
2. New `grpc_servers` config + a `spawn_listener()` branch in
   `wezterm-mux-server/src/main.rs` (tonic `serve_with_incoming` over a
   `wezterm_uds::UnixListener`; reuse `safely_create_sock_path`).
3. New `GrpcClientDomain` implementing `mux/src/domain.rs::Domain`.
4. New client config + connect variant beside `ClientDomainConfig::{Unix,Tls,Ssh}`
   (`wezterm-client/src/client.rs:648`).
5. SSH adapter: re-point `wezterm/src/cli/proxy.rs` / adapt `SshStream` so tonic
   connects over the SSH stdio byte stream.
6. Build: `tonic`/`prost` + `build.rs` codegen using vendored protoc or `protox`
   (no system protobuf-compiler).
7. A separate proto IDL crate, kept apart from the legacy `codec` crate.

Follow-up:
- The in-process runtime-bridge spike (Experiment 9) settles whether the
  tokio↔main-thread-`promise` hand-off is clean in practice.

## Result Template

Each completed experiment should append a short result section:

```markdown
### Result

Date:
Branch/prototype:
Verdict: pass | fail | inconclusive

Findings:
- ...

Follow-up:
- ...
```

## Suggested Order

1. Common Prototype
2. Unix Domain Socket Transport
3. SSH Proxy Transport
4. Streaming Latency and Throughput
5. Flow Control and Slow Clients
6. Snapshot and Blob Payloads
7. Reconnect and Failure Behavior
8. Cross-Stream Race Semantics
9. Tooling and Alternative Clients
10. WezTerm Runtime Integration

The first three experiments decide whether gRPC can fit the required deployment
shape. The rest decide whether it remains pleasant and reliable under mux-like
load.
