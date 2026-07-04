//! Shared protobuf schema for the experimental gRPC mux.
//!
//! This crate intentionally starts as the owner of the `.proto` file without
//! generating Rust bindings yet. The implementation crates can add tonic/prost
//! codegen once the server/client scaffolding is ready.

pub const STREAMING_MUX_PROTO_PATH: &str =
    "proto/wezterm/streaming_mux/v1/streaming_mux.proto";

pub const STREAMING_MUX_PROTO: &str =
    include_str!("../proto/wezterm/streaming_mux/v1/streaming_mux.proto");
