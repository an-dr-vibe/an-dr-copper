fn main() {
    if let Err(err) = copperd::cli::run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
