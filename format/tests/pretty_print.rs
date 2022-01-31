extern crate difference;
extern crate env_logger;
#[macro_use]
extern crate pretty_assertions;
extern crate termcolor;

extern crate gluon;
extern crate gluon_base as base;
extern crate gluon_format as format;

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use difference::{Changeset, Difference};

use gluon::{Compiler, VmBuilder};

fn assert_diff(text1: &str, text2: &str) -> io::Result<()> {
    let Changeset { diffs, .. } = Changeset::new(text1, text2, "\n");

    use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

    let mut t = StandardStream::stdout(ColorChoice::Auto);

    for i in 0..diffs.len() {
        match diffs[i] {
            Difference::Same(ref x) => {
                t.reset()?;
                writeln!(t, " {}", x)?;
            }
            Difference::Add(ref x) => {
                match diffs[i - 1] {
                    Difference::Rem(ref y) => {
                        t.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                        write!(t, "+")?;
                        let Changeset { diffs, .. } = Changeset::new(y, x, " ");
                        for c in diffs {
                            match c {
                                Difference::Same(ref z) => {
                                    t.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                                    write!(t, "{}", z)?;
                                    write!(t, " ")?;
                                }
                                Difference::Add(ref z) => {
                                    t.set_color(ColorSpec::new().set_fg(Some(Color::White)))?;
                                    t.set_color(ColorSpec::new().set_bg(Some(Color::Green)))?;
                                    write!(t, "{}", z)?;
                                    t.reset()?;
                                    write!(t, " ")?;
                                }
                                _ => (),
                            }
                        }
                        writeln!(t)?;
                    }
                    _ => {
                        t.set_color(
                            ColorSpec::new()
                                .set_fg(Some(Color::Green))
                                .set_intense(true),
                        )?;
                        writeln!(t, "+{}", x)?;
                    }
                };
            }
            Difference::Rem(ref x) => {
                t.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
                writeln!(t, "-{}", x)?;
            }
        }
    }
    t.reset()?;
    t.flush()?;
    Ok(())
}

macro_rules! assert_diff {
    ($lhs:expr, $rhs:expr, $sep:expr, $distance:expr) => {

        assert_diff($lhs, $rhs).unwrap();
    };
}

fn format_expr(expr: &str) -> gluon::Result<String> {
    let mut compiler = Compiler::new();
    let thread = VmBuilder::new()
        .import_paths(Some(vec!["..".into()]))
        .build();
    format::format_expr(&mut compiler, &thread, "test", expr)
}

fn format_expr_expanded(expr: &str) -> gluon::Result<String> {
    let mut compiler = Compiler::new();
    let thread = VmBuilder::new()
        .import_paths(Some(vec!["..".into()]))
        .build();
    format::Formatter { expanded: true }.format_expr(&mut compiler, &thread, "test", expr)
}

fn test_format(name: &str) {
    let _ = env_logger::try_init();

    let mut contents = String::new();
    File::open(Path::new("../").join(name))
        .unwrap()
        .read_to_string(&mut contents)
        .unwrap();

    let mut compiler = Compiler::new();
    let thread = VmBuilder::new()
        .import_paths(Some(vec!["..".into()]))
        .build();
    let out_str = format::format_expr(&mut compiler, &thread, name, &contents)
        .unwrap_or_else(|err| panic!("{}", err));

    if contents != out_str {
        let args: Vec<_> = env::args().collect();
        let out_path = Path::new(&args[0][..])
            .parent()
            .and_then(|p| p.parent())
            .expect("folder")
            .join(Path::new(name).file_name().unwrap());
        File::create(out_path)
            .unwrap()
            .write_all(out_str.as_bytes())
            .unwrap();

        assert_diff!(&contents, &out_str, " ", 0);
    }
}

#[test]
fn bool() {
    test_format("std/bool.glu");
}

#[test]
fn char() {
    test_format("std/char.glu");
}

#[test]
fn function() {
    test_format("std/function.glu");
}

#[test]
fn map() {
    test_format("std/map.glu");
}

#[test]
fn option() {
    test_format("std/option.glu");
}

#[test]
fn prelude() {
    test_format("std/prelude.glu");
}

#[test]
fn result() {
    test_format("std/result.glu");
}

#[test]
fn state() {
    test_format("std/state.glu");
}

#[test]
fn stream() {
    test_format("std/stream.glu");
}

#[test]
fn string() {
    test_format("std/string.glu");
}

#[test]
fn test() {
    test_format("std/test.glu");
}

#[test]
fn types() {
    test_format("std/types.glu");
}

#[test]
fn unit() {
    test_format("std/unit.glu");
}

#[test]
fn writer() {
    test_format("std/writer.glu");
}

#[test]
fn parser() {
    test_format("std/parser.glu");
}

#[test]
fn random() {
    test_format("std/random.glu");
}

#[test]
fn repl() {
    test_format("repl/src/repl.glu");
}

#[test]
fn dont_add_newline_for_let_literal() {
    let expr = r#"
let x = 1
x
"#;
    assert_eq!(
        &format_expr(expr).unwrap(),
        r#"
let x = 1
x
"#
    );
}

#[test]
fn dont_lose_information_in_literals() {
    let expr = r#"
3.14 "\t\n\r\""
"#;
    assert_eq!(&format_expr(expr).unwrap(), expr);
}

#[test]
fn implicit_arg() {
    let expr = r#"
f ?32 ""
"#;
    assert_eq!(&format_expr(expr).unwrap(), expr);
}

#[test]
fn preserve_comment_between_let_in() {
    let expr = r#"
// test1
let x = 1
// test2
type Test = Int
// test3
1
// test4
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn preserve_whitespace_in_record() {
    let expr = r#"
{
    aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaax = 1,


    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbby = 2,
}
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn preserve_block_comments() {
    let expr = r#"
/* test */
let x = { field = f /* test */ 123 /* doc */ }
/* test */
x
"#;
    assert_eq!(&format_expr(expr).unwrap(), expr);
}

// TODO
#[test]
fn preserve_more_block_comments() {
    let expr = r#"
{ /* abc */ field /* abc */ = /* test */ 123 }
"#;
    assert_eq!(&format_expr(expr).unwrap(), expr);
}

#[test]
fn preserve_shebang_line() {
    let expr = r#"#!/bin/gluon
/* test */
let x = { field = f /* test */ 123 /* doc */ }
/* test */
x
"#;
    assert_eq!(&format_expr(expr).unwrap(), expr);
}

#[test]
fn nested_constructor_pattern() {
    let expr = r#"
match None with
| Some (Some x) -> x
| _ -> 123
"#;
    assert_eq!(&format_expr(expr).unwrap(), expr);
}

#[test]
fn long_pattern_match() {
    let expr = r#"
let {
    CCCCCCCCCCCCCC,
    aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa,
    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
} =
    test
123
"#;
    assert_eq!(&format_expr(expr).unwrap(), expr);
}

#[test]
fn preserve_comments_in_function_types() {
    let expr = r#"#!/bin/gluon
let x : /* first */ Int /* Int */ ->
        // Float
        Float /* last */ = ()
x
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn preserve_comments_app_types() {
    let expr = r#"#!/bin/gluon
let x : Test /* first */ Int
        // middle
        Float /* last */ = ()
x
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn preserve_doc_comments_in_record_types() {
    let expr = r#"#!/bin/gluon
type Test = {
    /// test
    field1 : Int,
    /**
     middle
    */
    field2 : Float
}
x
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn doc_comment_in_record_expr() {
    let expr = r#"
{
    /// test
    /// test
    field1 = 1,
}
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn preserve_comments_in_empty_record() {
    let expr = r#"
{
// 123
}
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn preserve_comments_in_record_base() {
    let expr = r#"
{
    // 123
    ..
    // abc
    test
/* x */
}
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn small_record_in_let() {
    let expr = r#"
let semigroup =
    { append }
()
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn do_expression() {
    let expr = r#"
do /* x1 */ x /* x2 */ = Some 1
// test
test abc 1232 ""
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn if_else_multiple() {
    let expr = r#"
if x
then y
else if z
then w
else 0
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn comments_in_block_exprs() {
    let expr = r#"
// test
test 123

// test1

// test1

abc ""
// test2
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
#[ignore] // TODO
fn function_type_with_comments() {
    let expr = r#"
type Handler a =
    // Success continuation
    (a -> HttpState -> IO Response)
    // Failure continuation
    -> (Failure -> HttpState -> IO Response)
    -> IO Response
()
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn variant_type() {
    let expr = r#"
type TestCase a =
    | LoooooooooooooooooongTest String (() -> std.test.Test a)
    | LoooooooooooooooooooooooongGroup String (Array (std.test.TestCase a))
()
"#;
    assert_diff!(
        &format_expr(expr).unwrap_or_else(|err| panic!("{}", err)),
        expr,
        " ",
        0
    );
}

#[test]
fn multiline_string() {
    let expr = r#"
let x = "abc
        123
    "
x
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn derive() {
    let expr = r#"
#[derive(Show)]
type Test =
    | Test
Test
"#;
    assert_diff!(&format_expr(expr).unwrap(), expr, " ", 0);
}

#[test]
fn derive_expanded() {
    let expr = r#"
#[derive(Show)]
type Test =
    | Test
Test
"#;
    let expected = r#"
#[derive(Show)]
type Test =
    | Test
let show =
    let show_ x =
        match x with
        | Test -> "Test"
    { show }
Test
"#;
    assert_diff!(&format_expr_expanded(expr).unwrap(), expected, " ", 0);
}
