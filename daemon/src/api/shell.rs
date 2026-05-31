#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
pub struct ShellResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(cmd: &str, args: &[String]) -> ShellResult {
    let mut command = std::process::Command::new(cmd);
    command.args(args);
    apply_hidden_window(&mut command);
    match command.output() {
        Ok(output) => ShellResult {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(err) => ShellResult {
            code: -1,
            stdout: String::new(),
            stderr: err.to_string(),
        },
    }
}

pub fn which(binary: &str) -> Option<String> {
    let locator = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let mut command = std::process::Command::new(locator);
    command.arg(binary);
    apply_hidden_window(&mut command);
    let output = command.output().ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout);
        Some(path.lines().next().unwrap_or("").trim().to_string())
    } else {
        None
    }
}

fn apply_hidden_window(command: &mut std::process::Command) {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

#[cfg(test)]
mod tests {
    use super::{run, which};

    #[test]
    fn run_captures_stdout() {
        #[cfg(target_os = "windows")]
        let result = run("cmd", &["/c".to_string(), "echo hello".to_string()]);
        #[cfg(not(target_os = "windows"))]
        let result = run("echo", &["hello".to_string()]);
        assert_eq!(result.code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn run_missing_command_returns_nonzero() {
        let result = run("definitely-not-a-real-binary-xyz-12345", &[]);
        assert_ne!(result.code, 0);
    }

    #[test]
    fn which_finds_known_binary() {
        #[cfg(target_os = "windows")]
        let found = which("cmd");
        #[cfg(not(target_os = "windows"))]
        let found = which("sh");
        assert!(found.is_some(), "cmd/sh should be findable via PATH");
    }

    #[test]
    fn which_returns_none_for_missing_binary() {
        assert!(which("definitely-not-a-real-binary-xyz-12345").is_none());
    }
}
