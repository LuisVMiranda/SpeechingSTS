use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon.rc");
    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }
    let resource =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("speeching-icon.res");
    let rc = env::var_os("RC")
        .map(PathBuf::from)
        .or_else(find_windows_rc)
        .expect("Windows resource compiler not found; install the Windows SDK or set RC");
    let status = Command::new(rc)
        .arg("/nologo")
        .arg(format!("/fo{}", resource.display()))
        .arg("assets/icon.rc")
        .status()
        .expect("Failed to start the Windows resource compiler");
    assert!(status.success(), "Failed to compile the application icon");
    println!("cargo:rustc-link-arg={}", resource.display());
}

fn find_windows_rc() -> Option<PathBuf> {
    let base = PathBuf::from(env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
    let mut versions: Vec<_> = std::fs::read_dir(base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    versions.sort();
    versions
        .into_iter()
        .rev()
        .map(|p| p.join("x64/rc.exe"))
        .find(|p| p.is_file())
}
