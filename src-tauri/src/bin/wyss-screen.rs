//! The parser helper: one terminal's screen, in a process of its own. Started by the host with its
//! standard input and output as the only channel; it applies its isolation before it reads a byte.
//!
//!   wyss-screen [--require <layers>]       serve (the host's normal use)
//!   wyss-screen --sandbox-probe <layers> <action>    apply, try one forbidden thing, print the answer

use std::io::{BufReader, BufWriter};
use wyss_console::screen::{
    helper::{self, Exit},
    ipc::layer,
    sandbox,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--sandbox-probe") {
        let (Some(layers), Some(action)) =
            (args.get(1).and_then(|v| v.parse::<u8>().ok()), args.get(2))
        else {
            std::process::exit(2);
        };
        println!("{}", helper::probe(layers, action));
        return;
    }
    let Some(require) = helper::parse_require(&args) else {
        eprintln!("wyss-screen: usage: wyss-screen [--require <layers>]");
        std::process::exit(2);
    };
    // The host starts this program from a verified descriptor, so the kernel names the process
    // after the descriptor ("4"); it names itself, for process listings and for whoever has to
    // find the right process to end.
    // SAFETY: PR_SET_NAME reads a NUL-terminated string of at most 16 bytes.
    unsafe {
        nix::libc::prctl(nix::libc::PR_SET_NAME, c"wyss-screen".as_ptr(), 0, 0, 0);
    }
    // Every layer is attempted; the required ones decide whether this process may go on.
    let applied = match sandbox::apply(layer::ALL, require) {
        Ok(a) => a,
        Err(_) => std::process::exit(Exit::Sandbox.code()),
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = BufReader::with_capacity(64 * 1024, stdin.lock());
    let mut output = BufWriter::with_capacity(64 * 1024, stdout.lock());
    std::process::exit(helper::serve(&mut input, &mut output, applied).code());
}
