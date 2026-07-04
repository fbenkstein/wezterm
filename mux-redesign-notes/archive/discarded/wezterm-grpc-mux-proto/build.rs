use std::path::Path;

const PROTO_ROOT: &str = "proto";
const STREAMING_MUX_PROTO: &str = "proto/wezterm/streaming_mux/v1/streaming_mux.proto";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={STREAMING_MUX_PROTO}");

    let file_descriptor_set = protox::compile(
        [Path::new(STREAMING_MUX_PROTO)],
        [Path::new(PROTO_ROOT)],
    )?;

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_fds(file_descriptor_set)?;

    Ok(())
}
