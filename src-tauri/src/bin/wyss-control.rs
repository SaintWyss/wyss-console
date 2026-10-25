use wyss_console::contracts::control::ControlError;

/// Startup refused for a reason another attempt cannot change: the authority already
/// runs, or a path it needs is not safe. A service manager is told not to try again.
const EXIT_WILL_NOT_START: i32 = 2;

fn main() {
    let mut arguments = std::env::args_os().skip(1).peekable();
    if arguments.peek().is_some_and(|a| a == "--prepare-v9-copy") {
        arguments.next();
        if wyss_console::f8::v9::prepare_copy_arguments(arguments).is_err() {
            eprintln!("wyss-control: offline V9 preparation refused");
            std::process::exit(EXIT_WILL_NOT_START);
        }
        return;
    }
    // No arguments: the installed protocol-1 authority. Arguments: protocol 2, which the
    // caller asked for by name and which never falls back to protocol 1.
    let started = if arguments.peek().is_none() {
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
