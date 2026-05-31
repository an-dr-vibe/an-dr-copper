#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    let result = if std::env::args_os().len() > 1 {
        copperd::cli::run().map_err(|err| err.to_string())
    } else {
        copperd::daemon::run_daemon(copperd::daemon::DaemonConfig::default())
            .map_err(|err| err.to_string())
    };

    if let Err(err) = result {
        copperd::logging::error(format!("error: {err}"));
        std::process::exit(1);
    }
}
