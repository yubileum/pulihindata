fn main() {
    // Raw disk access on Windows needs elevation: ask for it up front via
    // the manifest instead of failing with "access denied" later.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        use embed_manifest::manifest::ExecutionLevel;
        embed_manifest::embed_manifest(
            embed_manifest::new_manifest("Pulih.RecoverDeletedFiles")
                .requested_execution_level(ExecutionLevel::RequireAdministrator),
        )
        .expect("unable to embed manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
