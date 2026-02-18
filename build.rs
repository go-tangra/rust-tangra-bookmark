use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_dir = PathBuf::from("proto");

    // Include path for well-known types shipped alongside protoc
    let protoc_include = protoc_include_dir();

    let include_dirs: Vec<PathBuf> = if let Some(ref inc) = protoc_include {
        vec![proto_dir.clone(), inc.clone()]
    } else {
        vec![proto_dir.clone()]
    };

    let bookmark_protos = &[
        "proto/bookmark/service/v1/bookmark.proto",
        "proto/bookmark/service/v1/permission.proto",
        "proto/bookmark/service/v1/backup.proto",
    ];

    let registration_proto = "proto/common/service/v1/module_registration.proto";

    // Compile bookmark service protos
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .file_descriptor_set_path(
            PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("bookmark_descriptor.bin"),
        )
        .compile_protos(bookmark_protos, &include_dirs)?;

    // Compile module registration proto (client only — we call LCM, not serve it)
    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile_protos(&[registration_proto], &include_dirs)?;

    Ok(())
}

/// Attempt to find the protoc well-known types include directory.
fn protoc_include_dir() -> Option<PathBuf> {
    // Check PROTOC_INCLUDE env var first
    if let Ok(dir) = std::env::var("PROTOC_INCLUDE") {
        let p = PathBuf::from(dir);
        if p.exists() {
            return Some(p);
        }
    }

    // Try sibling include/ dir relative to protoc binary
    if let Ok(protoc) = std::env::var("PROTOC") {
        let p = PathBuf::from(protoc);
        if let Some(parent) = p.parent().and_then(|p| p.parent()) {
            let inc = parent.join("include");
            if inc.exists() {
                return Some(inc);
            }
        }
    }

    // Common system paths
    for path in &[
        "/usr/include",
        "/usr/local/include",
    ] {
        let p = PathBuf::from(path).join("google/protobuf");
        if p.exists() {
            return Some(PathBuf::from(path));
        }
    }

    None
}
