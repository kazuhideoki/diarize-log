use std::path::Path;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

    for candidate in swift_runtime_candidates() {
        if Path::new(&candidate).exists() {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{candidate}");
        }
    }
}

fn swift_runtime_candidates() -> Vec<String> {
    let mut candidates = Vec::new();

    if let Ok(output) = Command::new("xcode-select").arg("-p").output()
        && output.status.success()
    {
        let developer_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !developer_dir.is_empty() {
            candidates.push(format!(
                "{developer_dir}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx"
            ));
            candidates.push(format!(
                "{developer_dir}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx"
            ));
        }
    }

    candidates.push("/Library/Developer/CommandLineTools/usr/lib/swift/macosx".to_string());
    candidates.push("/Library/Developer/CommandLineTools/usr/lib/swift-5.5/macosx".to_string());
    candidates
}
