fn main() {
    let dir = std::path::Path::new("web/dist");
    if !dir.exists() {
        std::fs::create_dir_all(dir).expect("failed to create web/dist/");
    }

    // Compile gRPC proto for zc serve mode
    tonic_build::compile_protos("proto/zeroclaw.proto").expect("failed to compile proto");
}
