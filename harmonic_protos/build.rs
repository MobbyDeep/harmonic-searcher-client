use tonic_build::configure;

fn main() {
    configure()
        .compile_protos(
            &[
                "protos/auth.proto",
                "protos/bundle.proto",
                "protos/packet.proto",
                "protos/searcher.proto",
                "protos/shared.proto",
            ],
            &["protos"],
        )
        .unwrap();
}
