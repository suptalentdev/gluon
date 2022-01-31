extern crate env_logger;

extern crate gluon_base as base;
extern crate gluon_parser as parser;
extern crate gluon_check as check;

use base::types::{Field, Type};

mod support;
use support::{intern, typ};

macro_rules! assert_pass {
    ($e: expr) => {{
        if !$e.is_ok() {
            panic!("assert_pass: {}", $e.unwrap_err());
        }
    }};
}

#[test]
fn infer_fields() {
    let _ = env_logger::init();

    let text = r#"
let f vec = vec.x #Int+ vec.y
f
"#;
    let result = support::typecheck(text);
    let record = Type::record(vec![],
                              vec![Field {
                                       name: intern("x"),
                                       typ: typ("Int"),
                                   },
                                   Field {
                                       name: intern("y"),
                                       typ: typ("Int"),
                                   }]);
    assert_eq!(result.map(support::close_record),
               Ok(Type::function(vec![record], typ("Int"))));
}

#[test]
fn infer_additional_fields() {
    let _ = env_logger::init();

    let text = r#"
let f vec = vec.x #Int+ vec.y
f { x = 1, y = 2, z = 3 }
"#;
    let result = support::typecheck(text);
    assert_eq!(result, Ok(typ("Int")));
}

#[test]
fn field_access_on_record_with_type() {
    let _ = env_logger::init();

    let text = r#"
type Test = Int
let record = { Test, x = 1, y = "" }
record.y
"#;
    let result = support::typecheck(text);
    assert_eq!(result, Ok(typ("String")));
}

#[test]
fn if_else_different_records() {
    let _ = env_logger::init();

    let text = r#"
if True then
    { y = "" }
else
    { x = 1 }
"#;
    let result = support::typecheck(text);
    assert!(result.is_err());
}

#[test]
fn missing_field() {
    let _ = env_logger::init();

    let text = r#"
let f vec = vec.x #Int+ vec.y
f { x = 1 }
"#;
    let result = support::typecheck(text);

    assert!(result.is_err());
}
