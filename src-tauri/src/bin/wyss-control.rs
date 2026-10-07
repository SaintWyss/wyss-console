fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!("wyss-control: invalid_request");
        std::process::exit(1);
    }
    if let Err(error) = wyss_console::control::run_from_environment() {
        eprintln!("wyss-control: {error}");
        std::process::exit(1);
    }
}
