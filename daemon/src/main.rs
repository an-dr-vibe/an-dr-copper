fn main() {
    if let Err(err) = copperd::run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
