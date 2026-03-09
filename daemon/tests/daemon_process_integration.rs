use assert_cmd::Command;
use predicates::str::contains;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command as StdCommand, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn free_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local addr");
    format!("127.0.0.1:{}", addr.port())
}

fn wait_until_healthy(bind_addr: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
        let ok = cmd
            .args(["daemon", "health", "--bind-addr", bind_addr])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if ok {
            return;
        }
        thread::sleep(Duration::from_millis(80));
    }
    panic!("daemon did not become healthy within {:?}", timeout);
}

#[test]
fn daemon_process_handles_lifecycle_commands() {
    let bind_addr = free_addr();
    let daemon_ui_bind = free_addr();
    let extensions_dir = repo_root().join("extensions");
    let bin = assert_cmd::cargo::cargo_bin!("copperd");

    let mut daemon = StdCommand::new(bin)
        .args([
            "daemon",
            "run",
            "--bind-addr",
            &bind_addr,
            "--extensions-dir",
            extensions_dir.to_str().expect("utf-8 path"),
            "--reload-interval-ms",
            "100",
        ])
        .env("COPPERD_DISABLE_TRAY", "1")
        .env("COPPERD_DAEMON_UI_BIND", daemon_ui_bind)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemon");

    wait_until_healthy(&bind_addr, Duration::from_secs(6));

    let mut list_cmd = Command::new(bin);
    list_cmd
        .args(["daemon", "list", "--bind-addr", &bind_addr])
        .assert()
        .success()
        .stdout(contains("extensions listed"));

    let mut verify_cmd = Command::new(bin);
    verify_cmd
        .args(["daemon", "verify", "--bind-addr", &bind_addr])
        .assert()
        .success()
        .stdout(contains("verified"));

    let mut trigger_cmd = Command::new(bin);
    trigger_cmd
        .args([
            "daemon",
            "trigger",
            "desktop-torrent-organizer",
            "--action",
            "move-torrents",
            "--bind-addr",
            &bind_addr,
        ])
        .assert()
        .success()
        .stdout(contains("trigger prepared"));

    let mut windows_trigger_cmd = Command::new(bin);
    let windows_assert = windows_trigger_cmd.args([
        "daemon",
        "trigger",
        "windows-display-manager",
        "--action",
        "status",
        "--bind-addr",
        &bind_addr,
    ]);
    if cfg!(target_os = "windows") {
        windows_assert
            .assert()
            .success()
            .stdout(contains("trigger prepared"))
            .stdout(contains("hostExecution"));
    } else {
        windows_assert
            .assert()
            .failure()
            .stderr(contains("only supported on Windows"));
    }

    let mut reload_cmd = Command::new(bin);
    reload_cmd
        .args(["daemon", "reload", "--bind-addr", &bind_addr])
        .assert()
        .success()
        .stdout(contains("reloaded"));

    let mut shutdown_cmd = Command::new(bin);
    shutdown_cmd
        .args(["daemon", "shutdown", "--bind-addr", &bind_addr])
        .assert()
        .success()
        .stdout(contains("shutdown signal accepted"));

    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if let Some(status) = daemon.try_wait().expect("try_wait") {
            assert!(status.success(), "daemon exited unsuccessfully: {status}");
            break;
        }
        if Instant::now() >= deadline {
            let _ = daemon.kill();
            panic!("daemon process did not stop after shutdown");
        }
        thread::sleep(Duration::from_millis(80));
    }
}
