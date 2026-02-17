fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_client(true)
        .build_server(false)
        .compile_protos(
            &[
                "proto/auth.proto",
                "proto/searcher.proto",
                "proto/bundle.proto",
                "proto/packet.proto",
                "proto/shared.proto",
            ],
            &["proto"],
        )?;

    println!("cargo:rerun-if-changed=proto");
    Ok(())
}
