//! Command line of `wyss-console`, as desktop launchers call a terminal:
//!
//! - `-e PROGRAM [ARGS…]` (or `-- PROGRAM [ARGS…]`): run it in a new tab; the tab closes when it ends.
//!   A single argument with spaces (`-e "htop -d 5"`) is taken as a shell line.
//! - `--workdir DIR`, `--working-directory DIR` (also `=DIR`): folder of the new tab.
//! - `--no-terminal`: open the window without creating a tab when there is nothing to restore.
//! - `N` (a number): tabs to open when there is nothing to restore.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub tabs: Option<usize>,
    pub workdir: Option<String>,
    pub command: Option<Vec<String>>,
    pub no_terminal: bool,
}

impl Request {
    /// Whether it asks for a specific tab (folder or program) rather than just the window.
    pub fn wants_tab(&self) -> bool {
        self.workdir.is_some() || self.command.is_some()
    }
}

/// `args` without the program name.
pub fn parse<S: AsRef<str>>(args: &[S]) -> Result<Request, String> {
    let mut r = Request::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_ref();
        match a {
            "-e" | "-x" | "--command" | "--" => {
                let rest: Vec<String> = args[i + 1..].iter().map(|s| s.as_ref().to_owned()).collect();
                if rest.is_empty() {
                    return Err(format!("{a} necesita un programa"));
                }
                r.command = Some(rest);
                break;
            }
            "--workdir" | "--working-directory" | "-w" => {
                i += 1;
                let dir = args.get(i).ok_or_else(|| format!("{a} necesita una carpeta"))?;
                r.workdir = Some(dir.as_ref().to_owned());
            }
            "--no-terminal" | "--sin-terminal" => r.no_terminal = true,
            _ => {
                if let Some(dir) = a.strip_prefix("--workdir=").or_else(|| a.strip_prefix("--working-directory=")) {
                    r.workdir = Some(dir.to_owned());
                } else if let Ok(n) = a.parse::<usize>() {
                    r.tabs = Some(n.max(1));
                } else {
                    return Err(format!("opción desconocida: {a}"));
                }
            }
        }
        i += 1;
    }
    Ok(r)
}

/// What is typed into the new shell so the program replaces it (`exec`), so the tab closes when the
/// program ends. Each argument is single-quoted, which POSIX shells and fish read the same way.
pub fn typed(command: &[String]) -> Vec<u8> {
    let line = if command.len() == 1 && command[0].contains(char::is_whitespace) {
        command[0].clone()
    } else {
        command.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
    };
    format!("exec {line}\r").into_bytes()
}

fn quote(a: &str) -> String {
    if !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c)) {
        return a.to_owned();
    }
    format!("'{}'", a.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_the_default_window() {
        assert_eq!(parse::<&str>(&[]).unwrap(), Request::default());
        assert!(!Request::default().wants_tab());
    }

    #[test]
    fn dash_e_takes_the_rest_as_the_program() {
        let r = parse(&["--workdir", "/tmp", "-e", "htop", "-d", "5"]).unwrap();
        assert_eq!(r.workdir.as_deref(), Some("/tmp"));
        assert_eq!(r.command, Some(vec!["htop".into(), "-d".into(), "5".into()]));
        assert!(r.wants_tab());
        assert_eq!(parse(&["--", "ls", "-e"]).unwrap().command, Some(vec!["ls".into(), "-e".into()]));
    }

    #[test]
    fn workdir_forms_and_flags() {
        assert_eq!(parse(&["--working-directory=/a b"]).unwrap().workdir.as_deref(), Some("/a b"));
        assert_eq!(parse(&["-w", "/x"]).unwrap().workdir.as_deref(), Some("/x"));
        let r = parse(&["--no-terminal", "3"]).unwrap();
        assert!(r.no_terminal && r.tabs == Some(3));
    }

    #[test]
    fn bad_input_is_refused() {
        assert!(parse(&["-e"]).is_err());
        assert!(parse(&["--workdir"]).is_err());
        assert!(parse(&["--rara"]).is_err());
    }

    #[test]
    fn typed_quotes_each_argument_and_execs() {
        assert_eq!(typed(&["htop".into()]), b"exec htop\r");
        assert_eq!(typed(&["echo".into(), "it's a b".into()]), b"exec echo 'it'\\''s a b'\r");
        assert_eq!(typed(&["htop -d 5".into()]), b"exec htop -d 5\r");
        assert_eq!(typed(&["printf".into(), "".into()]), b"exec printf ''\r");
    }
}
