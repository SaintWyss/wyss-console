fn main() {
    if std::env::args_os().nth(1).is_some() {
        eprintln!("uso: wyss-host");
        std::process::exit(2);
    }
    std::process::exit(wyss_console::host::run());
}
