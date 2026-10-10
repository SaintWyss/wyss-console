use wyss_console::contracts::control::ControlError;

/// Startup refused for a reason another attempt cannot change: the authority already
/// runs, or a path it needs is not safe. A service manager is told not to try again.
const EXIT_WILL_NOT_START: i32 = 2;

fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!("wyss-control: invalid_request");
        std::process::exit(EXIT_WILL_NOT_START);
    }
    if let Err(error) = wyss_console::control::run_from_environment() {
        eprintln!("wyss-control: {error}");
        std::process::exit(
            if matches!(
                error,
                ControlError::AlreadyRunning | ControlError::UnsafePath
            ) {
                EXIT_WILL_NOT_START
            } else {
                1
            },
        );
    }
}
