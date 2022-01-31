#[macro_use]
extern crate pretty_assertions;

use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn fmt_repl() {
    let source = "src/repl.glu";

    let mut before = String::new();
    File::open(source)
        .unwrap()
        .read_to_string(&mut before)
        .unwrap();

    let status = Command::new("../target/debug/gluon")
        .args(&["fmt", source])
        .spawn()
        .expect("Could not find gluon executable")
        .wait()
        .unwrap();
    assert!(status.success());

    let mut after = String::new();
    File::open(source)
        .unwrap()
        .read_to_string(&mut after)
        .unwrap();

    assert_eq!(before, after);
}

#[test]
fn issue_365_run_io_from_command_line() {
    let path = env::args().next().unwrap();
    let gluon_path = Path::new(&path[..])
        .parent()
        .and_then(|p| p.parent())
        .expect("folder")
        .join("gluon");
    let output = Command::new(&*gluon_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .arg("tests/print.glu")
        .output()
        .unwrap_or_else(|err| {
            panic!("{}\nWhen opening `{}`", err, gluon_path.display())
        });

    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "123\n");
}
