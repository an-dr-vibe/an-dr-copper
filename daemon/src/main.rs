#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    if let Err(err) = copperd::daemon::run_daemon(copperd::daemon::DaemonConfig::default()) {
        copperd::logging::error(format!("error: {err}"));
        std::process::exit(1);
    }
}
