# wezterm-grpc-mux-proto

Shared protobuf schema for the experimental gRPC mux redesign.

The authoritative protocol source is:

```text
proto/wezterm/streaming_mux/v1/streaming_mux.proto
```

Rust code generation is intentionally not wired up yet. This crate first gives
the protocol a stable home so the schema can be validated and iterated before
the client/server scaffolding depends on generated bindings.
