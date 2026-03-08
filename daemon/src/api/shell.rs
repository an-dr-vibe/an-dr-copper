#[derive(Debug, Clone)]
pub struct ShellResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(_cmd: &str, _args: &[String]) -> ShellResult {
    ShellResult {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn run_returns_stubbed_success_result() {
        let result = run("echo", &["ok".to_string()]);
        assert_eq!(result.code, 0);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }
}
