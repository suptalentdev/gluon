extern crate env_logger;
#[macro_use]
extern crate serde_derive;

extern crate gluon;

use gluon::base::types::{ArcType, Field, Type};
use gluon::base::symbol::Symbol;
use gluon::vm::api::VmType;
use gluon::vm::api::de::De;
use gluon::vm::thread::Thread;
use gluon::{Compiler, new_vm};

#[test]
fn bool() {
    let _ = env_logger::init();

    let thread = new_vm();
    let (De(b), _) = Compiler::new()
        .run_expr::<De<bool>>(&thread, "test", "True")
        .unwrap_or_else(|err| panic!("{}", err));
    assert_eq!(b, true);
}

#[derive(Debug, PartialEq, Deserialize)]
struct Record {
    test: i32,
    string: String,
}

impl VmType for Record {
    type Type = Self;

    fn make_type(thread: &Thread) -> ArcType {
        Type::poly_record(
            vec![],
            vec![
                Field {
                    name: Symbol::from("test"),
                    typ: i32::make_type(thread),
                },
                Field {
                    name: Symbol::from("string"),
                    typ: str::make_type(thread),
                },
            ],
            Type::hole(),
        )
    }
}

#[test]
fn record() {
    let _ = env_logger::init();

    let thread = new_vm();
    let (De(record), _) = Compiler::new()
        .run_expr::<De<Record>>(&thread, "test", r#" { test = 1, string = "test" } "#)
        .unwrap_or_else(|err| panic!("{}", err));
    assert_eq!(
        record,
        Record {
            test: 1,
            string: "test".to_string(),
        }
    );
}

#[test]
fn option() {
    let _ = env_logger::init();

    let thread = new_vm();
    let (De(opt), _) = Compiler::new()
        .run_expr::<De<Option<f64>>>(&thread, "test", r#" Some 1.0 "#)
        .unwrap_or_else(|err| panic!("{}", err));
    assert_eq!(opt, Some(1.0));
}

#[test]
fn partial_record() {
    let _ = env_logger::init();

    let thread = new_vm();
    let (De(record), _) = Compiler::new()
        .run_expr::<De<Record>>(
            &thread,
            "test",
            r#" { test = 1, extra = 1.0, string = "test", } "#,
        )
        .unwrap_or_else(|err| panic!("{}", err));
    assert_eq!(
        record,
        Record {
            test: 1,
            string: "test".to_string(),
        }
    );
}
