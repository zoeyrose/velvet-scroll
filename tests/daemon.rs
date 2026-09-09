//! Real process/socket/config tests. These never select or grab a physical device.
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Session {
    root: PathBuf,
    child: Child,
}
impl Session {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "velvet-scroll-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("runtime")).unwrap();
        fs::set_permissions(root.join("runtime"), fs::Permissions::from_mode(0o700)).unwrap();
        let child = Self::command(&root)
            .arg("daemon")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut session = Self { root, child };
        let start = Instant::now();
        loop {
            if session.cli(&["status", "--json"]).status.success() {
                return session;
            }
            assert!(
                session.child.try_wait().unwrap().is_none(),
                "daemon exited unexpectedly"
            );
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "daemon startup timed out"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn command(root: &Path) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_velvet-scroll"));
        cmd.env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env("XDG_CONFIG_HOME", root.join("config"));
        cmd
    }
    fn cli(&self, args: &[&str]) -> Output {
        Self::command(&self.root).args(args).output().unwrap()
    }
    fn status(&self) -> serde_json::Value {
        serde_json::from_slice(&self.cli(&["status", "--json"]).stdout).unwrap()
    }
    fn socket(&self) -> UnixStream {
        UnixStream::connect(self.root.join("runtime/velvet-scroll/control.sock")).unwrap()
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn controls_persist_and_stop_releases_singleton() {
    let mut session = Session::new();
    assert_eq!(session.status()["coast"], false);
    assert!(session.cli(&["disable"]).status.success());
    assert_eq!(session.status()["enabled"], false);
    assert!(session.cli(&["toggle"]).status.success());
    assert_eq!(session.status()["enabled"], true);
    assert!(session
        .cli(&["set", "acceleration", "4.5"])
        .status
        .success());
    assert!(session.cli(&["set", "coast", "on"]).status.success());
    assert!(!session
        .cli(&["set", "acceleration", "NaN"])
        .status
        .success());
    assert_eq!(session.status()["acceleration"], 4.5);
    let config: serde_json::Value = serde_json::from_slice(
        &fs::read(session.root.join("config/velvet-scroll/config.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(config["acceleration"], 4.5);
    assert_eq!(config["coast"], true);
    let duplicate = session.cli(&["daemon"]);
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("already running"));
    assert!(session.cli(&["stop"]).status.success());
    assert!(session.child.wait().unwrap().success());
    assert!(!session
        .root
        .join("runtime/velvet-scroll/control.sock")
        .exists());
    let offline = session.cli(&["status", "--json"]);
    assert!(!offline.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&offline.stdout).unwrap()["running"],
        false
    );
}
#[test]
fn stalled_and_malformed_clients_cannot_block_controls() {
    let session = Session::new();
    let mut stalled = session.socket();
    stalled.write_all(b"{\"command\":").unwrap();
    let start = Instant::now();
    assert!(session.cli(&["toggle"]).status.success());
    assert!(start.elapsed() < Duration::from_secs(1));
    let mut malformed = session.socket();
    malformed
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    malformed
        .write_all(b"{\"command\":\"toggle\",\"extra\":1}\n")
        .unwrap();
    let mut response = String::new();
    BufReader::new(malformed).read_line(&mut response).unwrap();
    let value: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert!(value["error"].as_str().unwrap().contains("Unknown"));
    let before = session.status()["enabled"].clone();
    let mut oversized = session.socket();
    let _ = oversized.write_all(&vec![b'x'; 9000]);
    drop(oversized);
    assert_eq!(session.status()["enabled"], before);
}
#[test]
fn termination_cleans_socket_and_validates_runtime_permissions() {
    let mut session = Session::new();
    unsafe {
        libc::kill(session.child.id() as i32, libc::SIGTERM);
    }
    assert!(session.child.wait().unwrap().success());
    assert!(!session
        .root
        .join("runtime/velvet-scroll/control.sock")
        .exists());
    fs::set_permissions(
        session.root.join("runtime"),
        fs::Permissions::from_mode(0o777),
    )
    .unwrap();
    let output = session.cli(&["daemon"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("without group/other write"));
}

#[test]
fn crash_restart_recovers_stale_socket_and_saved_preferences() {
    let mut session = Session::new();
    assert!(session
        .cli(&["set", "acceleration", "2.2"])
        .status
        .success());
    session.child.kill().unwrap();
    session.child.wait().unwrap();
    assert!(session
        .root
        .join("runtime/velvet-scroll/control.sock")
        .exists());
    session.child = Session::command(&session.root)
        .arg("daemon")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while !session.cli(&["status", "--json"]).status.success() {
        assert!(session.child.try_wait().unwrap().is_none());
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(session.status()["acceleration"], 2.2);
    assert!(session.status()["active_devices"]
        .as_array()
        .unwrap()
        .is_empty());
}
