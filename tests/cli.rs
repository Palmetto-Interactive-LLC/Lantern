use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;

/// Path to the lantern binary under test.
fn lantern_bin() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // deps
    path.pop(); // debug or release
    path.push("lantern");
    path
}

fn isolated_home() -> TempDir {
    TempDir::new().expect("create isolated home")
}

fn lantern_command(home: &TempDir) -> Command {
    let mut command = Command::new(lantern_bin());
    command.env("HOME", home.path());
    command
}

#[test]
fn test_cli_help() {
    let output = Command::new(lantern_bin())
        .arg("--help")
        .output()
        .expect("failed to execute lantern --help");

    assert!(output.status.success(), "lantern --help failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Lantern"), "help should mention Lantern");
    assert!(
        stdout.contains("relay"),
        "help should mention relay subcommand"
    );
    assert!(
        stdout.contains("startwork"),
        "help should mention startwork subcommand"
    );
}

#[test]
fn test_cli_version() {
    let output = Command::new(lantern_bin())
        .arg("--version")
        .output()
        .expect("failed to execute lantern --version");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = env!("CARGO_PKG_VERSION");
    assert!(
        stdout.contains(expected),
        "version output should contain {expected}"
    );
}

#[test]
fn test_status_command() {
    let home = isolated_home();
    let output = lantern_command(&home)
        .arg("status")
        .output()
        .expect("failed to execute lantern status");

    assert!(output.status.success(), "lantern status failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Lantern Relay Status") || stdout.contains("Machine:"),
        "status should print formatted status table"
    );
}

#[test]
fn test_relay_daemon_starts() {
    use std::thread;

    let home = isolated_home();
    let mut child = lantern_command(&home)
        .arg("relay")
        .arg("--machine")
        .arg("test-machine")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn lantern relay");

    // Give it time to initialize
    thread::sleep(Duration::from_secs(2));

    // Check it's still running
    match child.try_wait() {
        Ok(None) => {
            // Still running — good
        }
        Ok(Some(status)) => {
            let stderr = {
                let mut buf = String::new();
                use std::io::Read;
                child.stderr.take().unwrap().read_to_string(&mut buf).ok();
                buf
            };
            panic!("relay exited early: {:?}\nstderr: {}", status, stderr);
        }
        Err(e) => panic!("error checking relay status: {}", e),
    }

    // Gracefully terminate
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .arg("-INT")
            .arg(child.id().to_string())
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }

    let _ = child.kill();
    let _ = child.wait();
}
