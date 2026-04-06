use std::process::Command;

fn main() {
    // Always re-run so the timestamp updates on every build.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=FORCE_REBUILD");

    // Embed a human-readable build timestamp (e.g. "Apr  6 14:32").
    let output = Command::new("date")
        .arg("+%b %e %H:%M")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=PULSE_BUILD_TIME={}", output);
}
