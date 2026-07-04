# wezterm-grpc-mux-proto

Archived protobuf schema for the experimental gRPC mux redesign. This crate is
kept for reference and is not part of the active workspace.

The authoritative protocol source is:

```text
proto/wezterm/streaming_mux/v1/streaming_mux.proto
```

Rust bindings are generated at build time with `protox` and
`tonic-prost-build`, avoiding a host `protoc` dependency. Consumers can use the
`streaming_mux_v1` module for generated messages and service stubs.
