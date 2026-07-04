//! Shared protobuf schema and generated gRPC bindings for the experimental mux.

pub const STREAMING_MUX_PROTO_PATH: &str =
    "proto/wezterm/streaming_mux/v1/streaming_mux.proto";

pub const STREAMING_MUX_PROTO: &str =
    include_str!("../proto/wezterm/streaming_mux/v1/streaming_mux.proto");

pub mod wezterm {
    pub mod streaming_mux {
        pub mod v1 {
            tonic::include_proto!("wezterm.streaming_mux.v1");
        }
    }
}

pub use wezterm::streaming_mux::v1 as streaming_mux_v1;

#[cfg(test)]
mod tests {
    use super::streaming_mux_v1::{
        streaming_mux_client::StreamingMuxClient,
        streaming_mux_server::StreamingMuxServer,
    };

    #[test]
    fn generated_service_items_are_exported() {
        let _ = std::any::type_name::<StreamingMuxClient<tonic::transport::Channel>>();
        let _ = std::any::type_name::<StreamingMuxServer<()>>();
    }
}
