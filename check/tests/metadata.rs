
extern crate env_logger;

extern crate base;
extern crate parser;
extern crate check;

mod functions;
use functions::*;

use base::metadata::Metadata;
use check::metadata::*;

#[test]
fn propagate_metadata_let_in() {
    let _ = ::env_logger::init();
    let text = r#"
/// The identity function
let id x = x
id
"#;
    let (mut expr, result) = typecheck_expr(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let metadata = metadata(&(), &mut expr);
    assert_eq!(metadata, Metadata {
        comment: Some("The identity function".into()),
        module: Default::default(),
    });
}

#[test]
fn propagate_metadata_let_record() {
    let _ = ::env_logger::init();
    let text = r#"
/// The identity function
let id x = x
{ id }
"#;
    let (mut expr, result) = typecheck_expr(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let metadata = metadata(&(), &mut expr);
    assert_eq!(metadata.module.get("id"), Some(&Metadata {
        comment: Some("The identity function".into()),
        module: Default::default(),
    }));
}

#[test]
fn propagate_metadata_type_record() {
    let _ = ::env_logger::init();
    let text = r#"
/// A test type
type Test = Int
{ Test }
"#;
    let (mut expr, result) = typecheck_expr(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let metadata = metadata(&(), &mut expr);
    assert_eq!(metadata.module.get("Test"), Some(&Metadata {
        comment: Some("A test type".into()),
        module: Default::default(),
    }));
}
