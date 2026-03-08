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
