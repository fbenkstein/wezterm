# Mux Redesign Investigation Strands

Status tracker for the short investigations needed before the experimental mock
server can be scaffolded with confidence.

## 1. Proto Codegen Path

Status: **done**.

What is settled:

- Use pure-Rust `protox`; do not require a host `protoc` or
  `protobuf-compiler`.
- Prefer current tonic/prost tooling while this work is experimental, even if it
  raises WezTerm's current MSRV. Revisit MSRV only near an upstream contribution
  or release path.
- Current tonic splits protobuf integration into runtime `tonic-prost` and build
  crate `tonic-prost-build`; use that shape rather than the older
  `tonic-build`-only pattern.

Completed work:

- Add workspace dependencies for current `tonic`, `tonic-prost`, `prost`,
  `tonic-prost-build`, and `protox`.
- Add `wezterm-grpc-mux-proto/build.rs` that runs `protox::compile()` over
  `proto/wezterm/streaming_mux/v1/streaming_mux.proto`, then feeds the resulting
  descriptor set to `tonic_prost_build::configure().compile_fds(...)`.
- Update `wezterm-grpc-mux-proto/src/lib.rs` to expose the generated module via
  `tonic::include_proto!("wezterm.streaming_mux.v1")`, while keeping the raw
  schema constants useful for tooling.
- Run `cargo check -p wezterm-grpc-mux-proto` and fix any generated-code issues.
- Add a tiny compile-only test that references both `StreamingMuxClient` and
  `StreamingMuxServer` so missing service generation is caught early.

Done criteria:

- The schema crate generates tonic/prost bindings without host `protoc`.
- `cargo check -p wezterm-grpc-mux-proto` passes from a clean checkout.
- Generated module paths and public exports are clear enough for the future
  server crate to depend on.

## 2. Protocol Completeness For The Mock Server

Status: **open**.

Questions:

- Which RPCs need a real MVP response for a runnable mock server, and which
  should deliberately return `UNIMPLEMENTED`?
- What minimal control-stream handshake is required so a client can connect and
  discover that the server speaks the protocol?
- Are error codes and request identifiers sufficient for forwarded UI requests,
  layout update failures, duplicate persistent client IDs, and no-client cases?
- Do streaming sequence fields have enough definition for an implementation to
  stub them without painting us into a corner?

Done criteria:

- A small "MVP implemented vs UNIMPLEMENTED" table exists.
- The `.proto` has any missing request/response or error-shape fixes needed for
  that table.

## 3. Experimental Server Crate Shape

Status: **open**.

Questions:

- Exact crate name and binary name: likely `wezterm-grpc-mux-server`.
- Which existing workspace crates should be reused immediately for socket path
  handling, daemonization, logging, and proxying?
- Whether the first mock server can be a pure tokio binary, or whether it should
  start with the dedicated-tokio-thread plus channel bridge pattern proven in the
  viability notes.
- What compile feature or command-line flag should keep the experimental server
  side-by-side with the existing mux server?

Done criteria:

- Crate scaffolding plan lists dependencies, binary entry point, and runtime
  ownership.
- The plan identifies the minimum code to get `--help`, server startup, and
  graceful shutdown compiling.

## 4. Proxying And Auto-Daemonization

Status: **open**.

Questions:

- Which parts of the existing `wezterm-mux-server` proxy and daemonization code
  can be reused directly?
- What does "similar to existing server but minimized" require for local Unix
  socket startup, remote SSH proxying, and client-triggered daemon launch?
- Does gRPC over stdio need a separate `proxy-grpc` command or can the new binary
  expose one command whose byte stream is transport-agnostic?

Done criteria:

- File-level reuse plan for socket creation, proxy command, and daemon startup.
- Clear MVP command surface for local server, proxy mode, and client
  auto-daemonization.

## 5. Minimal Server State

Status: **open**.

Questions:

- What in-memory state is needed before real PTY integration: sessions, connected
  clients, persistent client IDs, focus membership, layout blobs, and pane
  summaries?
- Which state must exist to make basic methods useful, and which can wait until
  pane spawning is real?
- Do we need any on-disk state for the mock server, or can all persistence wait?

Done criteria:

- A minimal state struct sketch exists.
- Each basic RPC has a clear state read/write story or is marked
  `UNIMPLEMENTED`.

## 6. Test And Smoke Strategy

Status: **open**.

Questions:

- What is the first automated test: generated-code compile, in-process tonic
  server/client, or a CLI-level smoke test against a Unix socket?
- How should tests avoid depending on a host `protoc`?
- Which behavior is worth testing before PTY integration: handshake,
  duplicate-client handling, layout get/update, `UNIMPLEMENTED` status, and
  proxy byte transport?

Done criteria:

- A short test plan exists for the first server scaffold.
- At least one command verifies the proto crate and mock server crate together.
