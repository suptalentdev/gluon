#![cfg(feature = "regex")]
extern crate env_logger;
extern crate gluon;

use gluon::{new_vm, Compiler};

#[test]
fn regex_match() {
    let _ = ::env_logger::init();

    let thread = new_vm();
    let text = r#"
        let { (|>) } = import! "std/function.glu"
        let { not } = import! "std/bool.glu"
        let { unwrap_ok } = import! "std/result.glu"
        let { assert }  = import! "std/test.glu"

        let match_a = regex.new "a" |> unwrap_ok
        assert (regex.is_match match_a "a")
        assert (not (regex.is_match match_a "b"))
        let match_hello = regex.new "hello, .*" |> unwrap_ok
        regex.is_match match_hello "hello, world"
        "#;
    let result = Compiler::new()
        .run_expr_async::<bool>(&thread, "<top>", text)
        .sync_or_error();

    assert!(result.unwrap().0);
}

#[test]
fn regex_error() {
    let _ = ::env_logger::init();

    let thread = new_vm();
    let text = r#"
        let { (|>) } = import! "std/function.glu"
        let { unwrap_err } = import! "std/result.glu"

        regex.new ")" |> unwrap_err |> regex.error_to_string
        "#;
    let result = Compiler::new()
        .run_expr_async::<String>(&thread, "<top>", text)
        .sync_or_error();

    assert_eq!(
        result.unwrap().0,
        "Error parsing regex near \')\' at character offset 0: Unopened parenthesis."
    );
}
