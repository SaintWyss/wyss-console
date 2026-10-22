use wyss_console::contracts::control::ControlError;

/// Startup refused for a reason another attempt cannot change: the authority already
/// runs, or a path it needs is not safe. A service manager is told not to try again.
const EXIT_WILL_NOT_START: i32 = 2;

fn main() {
    let arguments = std::env::args_os().skip(1);
    // No arguments: the installed protocol-1 authority. Arguments: protocol 2, which the
    // caller asked for by name and which never falls back to protocol 1.
    let started = if arguments.len() == 0 {
        wyss_console::control::run_from_environment()
    } else {
        wyss_console::control::serve::run_from_arguments(arguments)
    };
    if let Err(error) = started {
        eprintln!("wyss-control: {error}");
        std::process::exit(
            if matches!(
                error,
                ControlError::AlreadyRunning
                    | ControlError::UnsafePath
                    | ControlError::InvalidRequest
                    | ControlError::StorageIncompatible
                    | ControlError::GateDenied
            ) {
                EXIT_WILL_NOT_START
            } else {
                1
            },
        );
    }
}
