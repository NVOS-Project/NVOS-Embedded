use std::fs;
const PROTO_DIR: &str = "./protos";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let entries: Vec<String> = fs::read_dir(PROTO_DIR)
        .expect("Failed to list proto directory")
        .filter_map(|entry| {
            let path = entry.expect("Failed to read entry").path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "proto") {
                Some(path.to_str().expect("Failed to read entry path").to_owned())
            }
            else {
                None
            }
        })
        .collect();

    if entries.is_empty() {
        println!("No proto files to compile, aborting.");
        return Ok(());
    }

    tonic_build::configure()
        .build_server(true)
        .build_transport(true)
        .build_client(false)
        .compile(&entries, &[PROTO_DIR])
        .unwrap_or_else(|err| panic!("protobuf compile error: {}", err));

    Ok(())
}