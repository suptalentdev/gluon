extern crate env_logger;

extern crate gluon_base as base;
extern crate gluon_parser as parser;

use base::ast::{SpannedExpr, EmptyEnv};
use base::error::Errors;
use parser::{parse_string, Error};

fn parse(text: &str) -> Result<SpannedExpr<String>, Errors<Error>> {
    parse_string(&mut EmptyEnv::new(), text)
        .map_err(|(_, err)| err)
}

#[test]
fn dont_stack_overflow_on_let_bindings() {
    let text = r#"
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in
let _ = 1
in 1
"#;
    parse(text).unwrap();
}
