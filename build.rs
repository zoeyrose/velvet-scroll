use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git_path(project_dir: &Path, name: &str) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-path", name])
        .current_dir(project_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    Some(if path.is_absolute() {
        path
    } else {
        project_dir.join(path)
    })
}

fn main() {
    let project_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-env-changed=PACKAGE_VERSION");
    println!("cargo:rerun-if-changed=scripts/version.sh");
    if project_dir.join(".git").is_file() {
        println!("cargo:rerun-if-changed=.git");
    }
    for name in ["HEAD", "index", "packed-refs", "refs"] {
        if let Some(path) = git_path(&project_dir, name) {
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    let output = Command::new("sh")
        .arg(project_dir.join("scripts/version.sh"))
        .current_dir(&project_dir)
        .output()
        .expect("failed to run scripts/version.sh");
    if !output.status.success() {
        panic!(
            "scripts/version.sh failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let version =
        String::from_utf8(output.stdout).expect("scripts/version.sh returned non-UTF-8 output");
    let version = version.trim();
    assert!(
        !version.is_empty(),
        "scripts/version.sh returned no version"
    );
    println!("cargo:rustc-env=VELVET_SCROLL_VERSION={version}");
}
