fn main() {
    use wyss_console::native::claude_activity as claude;
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref().and_then(std::ffi::OsStr::to_str) {
        Some(claude::LAUNCH) => {
            let error = claude::launch(&args.collect::<Vec<_>>());
            eprintln!("wyss-host: {error}");
            std::process::exit(126);
        }
        Some(claude::RECEIVE) => {
            claude::receive(&args.collect::<Vec<_>>(), std::io::stdin().lock());
            return;
        }
        _ => {}
    }
    if std::env::args_os().nth(1).is_some() {
        eprintln!("uso: wyss-host");
        std::process::exit(2);
    }
    std::process::exit(wyss_console::host::run());
}
