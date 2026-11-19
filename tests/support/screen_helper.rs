use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};
use wyss_console::screen::ipc::{self, IpcError, MAX_REQUEST, MAX_RESPONSE, PROTOCOL, Request, Response};

pub struct Helper {
    pub child: Child,
    pub to: ChildStdin,
    pub from: ChildStdout,
}

pub fn spawn(args: &[&str]) -> Helper {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wyss-screen"))
        .args(args)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the helper starts");
    let to = child.stdin.take().unwrap();
    let from = child.stdout.take().unwrap();
    Helper { child, to, from }
}

pub fn send_frame(writer: &mut impl Write, request: &Request) {
    ipc::write_frame(writer, &request.encode(), MAX_REQUEST).unwrap();
}

pub fn receive_frame(reader: &mut impl Read) -> Result<Option<Response>, IpcError> {
    ipc::read_frame(reader, MAX_RESPONSE)?
        .map(|frame| Response::decode(&frame))
        .transpose()
}

pub fn hello(rows: u16, cols: u16, scrollback_lines: u32, require: u8) -> Request {
    Request::Hello { version: PROTOCOL, rows, cols, scrollback_lines, width_table: 0, require }
}
