#[macro_use]
extern crate collect_mac;
extern crate either;
extern crate env_logger;

extern crate gluon_base as base;
extern crate gluon_check as check;
extern crate gluon_completion as completion;
extern crate gluon_parser as parser;

use base::ast::Argument;
use base::metadata::Metadata;
use base::metadata::{Comment, CommentType};
use base::pos::{BytePos, Span};
use base::types::{ArcType, Field, Type};

#[allow(unused)]
mod support;
use support::{intern, loc, typ, MockEnv};

fn line_comment<S>(s: S) -> Comment
where
    S: Into<String>,
{
    Comment {
        typ: CommentType::Line,
        content: s.into(),
    }
}

fn find_span_type(s: &str, pos: BytePos) -> Result<(Span<BytePos>, ArcType), ()> {
    let env = MockEnv::new();

    let (expr, result) = support::typecheck_expr(s);
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let extract = (completion::SpanAt, completion::TypeAt { env: &env });
    completion::completion(extract, expr.span, &expr, pos)
}

fn find_all_symbols(s: &str, pos: BytePos) -> Result<(String, Vec<Span<BytePos>>), ()> {
    let (expr, result) = support::typecheck_expr(s);
    assert!(result.is_ok(), "{}", result.unwrap_err());

    completion::find_all_symbols(expr.span, &expr, pos)
}

fn find_type(s: &str, pos: BytePos) -> Result<ArcType, ()> {
    find_span_type(s, pos).map(|t| t.1)
}

fn find_type_loc(s: &str, line: usize, column: usize) -> Result<ArcType, ()> {
    let pos = loc(s, line, column);
    find_span_type(s, pos).map(|t| t.1)
}

fn get_metadata(s: &str, pos: BytePos) -> Option<Metadata> {
    let env = MockEnv::new();

    let (expr, result) = support::typecheck_expr(s);
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let (_, metadata_map) = check::metadata::metadata(&env, &expr);
    completion::get_metadata(&metadata_map, expr.span, &expr, pos).cloned()
}

fn suggest_metadata(s: &str, pos: BytePos, name: &str) -> Option<Metadata> {
    let env = MockEnv::new();

    let (expr, _result) = support::typecheck_expr(s);

    let (_, metadata_map) = check::metadata::metadata(&env, &expr);
    completion::suggest_metadata(&metadata_map, &env, expr.span, &expr, pos, name).cloned()
}

#[test]
fn identifier() {
    let env = MockEnv::new();

    let (expr, result) = support::typecheck_expr("let abc = 1 in abc");
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let result = completion::find(&env, expr.span, &expr, BytePos::from(15));
    let expected = Ok(typ("Int"));
    assert_eq!(result, expected);

    let result = completion::find(&env, expr.span, &expr, BytePos::from(16));
    let expected = Ok(typ("Int"));
    assert_eq!(result, expected);

    let result = completion::find(&env, expr.span, &expr, BytePos::from(17));
    let expected = Ok(typ("Int"));
    assert_eq!(result, expected);

    let result = completion::find(&env, expr.span, &expr, BytePos::from(18));
    let expected = Ok(typ("Int"));
    assert_eq!(result, expected);
}

#[test]
fn literal_string() {
    let result = find_type(r#" "asd" "#, BytePos::from(1));
    let expected = Ok(typ("String"));

    assert_eq!(result, expected);
}

#[test]
fn in_let() {
    let result = find_type(
        r#"
let f x = 1
and g x = "asd"
1
"#,
        BytePos::from(25),
    );
    let expected = Ok(typ("String"));

    assert_eq!(result, expected);
}

#[test]
fn let_in_let() {
    let result = find_type(
        r#"
let f =
    let g y =
        123
    g
f
"#,
        BytePos::from(33),
    );
    let expected = Ok(typ("Int"));

    assert_eq!(result, expected);
}

#[test]
fn function_app() {
    let _ = env_logger::try_init();

    let text = r#"
let f x = f x
1
"#;
    let result = find_type(text, loc(text, 1, 10));
    let expected = Ok("a -> a0".to_string());

    assert_eq!(result.map(|typ| typ.to_string()), expected);
}

#[test]
fn binop() {
    let _ = env_logger::try_init();

    let env = MockEnv::new();

    let text = r#"
let (++) l r =
    l #Int+ 1
    r #Float+ 1.0
    l
1 ++ 2.0
"#;
    let (expr, result) = support::typecheck_expr(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let result = completion::find(&env, expr.span, &expr, loc(text, 5, 3));
    let expected = Ok(Type::function(vec![typ("Int"), typ("Float")], typ("Int")));
    assert_eq!(result, expected);

    let result = completion::find(&env, expr.span, &expr, loc(text, 5, 1));
    let expected = Ok(typ("Int"));
    assert_eq!(result, expected);

    let result = completion::find(&env, expr.span, &expr, loc(text, 5, 7));
    let expected = Ok(typ("Float"));
    assert_eq!(result, expected);
}

#[test]
fn field_access() {
    let _ = env_logger::try_init();

    let typ_env = MockEnv::new();

    let (expr, result) = support::typecheck_expr(
        r#"
let r = { x = 1 }
r.x
"#,
    );
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let result = completion::find(&typ_env, expr.span, &expr, BytePos::from(19));
    let expected = Ok(Type::record(
        vec![],
        vec![Field::new(intern("x"), typ("Int"))],
    ));
    assert_eq!(result.map(support::close_record), expected);

    let result = completion::find(&typ_env, expr.span, &expr, BytePos::from(22));
    let expected = Ok(typ("Int"));
    assert_eq!(result, expected);
}

#[test]
fn find_do_binding_type() {
    let _ = env_logger::try_init();

    let result = find_type_loc(
        r#"
type Option a = | None | Some a
let flat_map f x =
    match x with
    | Some y -> f y
    | None -> None

do x = Some 1
None
"#,
        7,
        4,
    );
    let expected = Ok("Int".to_string());

    assert_eq!(result.map(|typ| typ.to_string()), expected);
}

#[test]
fn parens_expr() {
    let _ = env_logger::try_init();

    let text = r#"
let id x = x
(id 1)
"#;
    let (expr, result) = support::typecheck_expr(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let env = MockEnv::new();
    let extract = (completion::SpanAt, completion::TypeAt { env: &env });

    let result = completion::completion(extract, expr.span, &expr, loc(text, 2, 0));
    let expected = Ok((Span::new(loc(text, 2, 0), loc(text, 2, 6)), Type::int()));
    assert_eq!(result, expected);

    let result = completion::completion(extract, expr.span, &expr, loc(text, 2, 2));
    let expected = Ok((
        Span::new(loc(text, 2, 1), loc(text, 2, 3)),
        Type::function(vec![Type::int()], Type::int()),
    ));
    assert_eq!(result, expected);
}

#[test]
fn suggest_pattern_at_record_brace() {
    let _ = env_logger::try_init();

    let text = r#"
let { x } = { x = 1 }
x
"#;

    let result = find_span_type(text, loc(text, 1, 5));
    let expected = Ok((
        Span::new(loc(text, 1, 4), loc(text, 1, 9)),
        Type::record(
            vec![],
            vec![Field {
                name: intern("x"),
                typ: Type::int(),
            }],
        ),
    ));
    assert_eq!(result, expected);
}

#[test]
fn in_record() {
    let _ = env_logger::try_init();

    let result = find_type(
        r#"
{
    test = 123,
    s = "asd"
}
"#,
        BytePos::from(15),
    );
    let expected = Ok(typ("Int"));

    assert_eq!(result, expected);
}

#[test]
fn record_constructor_field() {
    let _ = env_logger::try_init();

    let result = find_type(r#"{ test = 123 }"#, BytePos::from(4));
    let expected = Ok(typ("Int"));

    assert_eq!(result, expected);
}

#[test]
fn function_arg() {
    let _ = env_logger::try_init();

    let result = find_type(
        r#"
let f x = x #Int+ 1
""
"#,
        BytePos::from(8),
    );
    let expected = Ok(Type::int());

    assert_eq!(result, expected);
}

#[test]
fn lambda_arg() {
    let _ = env_logger::try_init();

    let text = r#"
let f : Int -> String -> String = \x y -> y
1.0
"#;
    let result = find_type(text, loc(text, 1, 37));
    let expected = Ok(Type::string());

    assert_eq!(result, expected);
}

#[test]
fn unit() {
    let _ = env_logger::try_init();

    let result = find_type("()", BytePos::from(1));
    let expected = Ok(Type::unit());

    assert_eq!(result, expected);
}

#[test]
fn metadata_at_variable() {
    let _ = env_logger::try_init();

    let text = r#"
/// test
let abc = 1
let abb = 2
abb
abc
"#;
    let result = get_metadata(text, BytePos::from(37));

    let expected = None;
    assert_eq!(result, expected);

    let result = get_metadata(text, BytePos::from(41));

    let expected = Some(Metadata {
        comment: Some(line_comment("test".to_string())),
        ..Metadata::default()
    });
    assert_eq!(result, expected);
}

#[test]
fn metadata_at_binop() {
    let _ = env_logger::try_init();

    let text = r#"
/// test
let (+++) x y = 1
1 +++ 3
"#;
    let result = get_metadata(text, BytePos::from(32));

    let expected = Some(Metadata {
        comment: Some(line_comment("test".to_string())),
        args: ["x:21", "y:23"]
            .iter()
            .map(|arg| Argument::explicit(intern(arg)))
            .collect(),
        ..Metadata::default()
    });
    assert_eq!(result, expected);
}

#[test]
fn metadata_at_field_access() {
    let _ = env_logger::try_init();

    let text = r#"
let module = {
        /// test
        abc = 1,
        abb = 2
    }
module.abc
"#;
    let result = get_metadata(text, BytePos::from(81));

    let expected = Some(Metadata {
        comment: Some(line_comment("test".to_string())),
        ..Metadata::default()
    });
    assert_eq!(result, expected);
}

#[test]
fn suggest_metadata_at_variable() {
    let _ = env_logger::try_init();

    let text = r#"
/// test
let abc = 1
let abb = 2
ab
"#;
    let result = suggest_metadata(text, BytePos::from(36), "abc");

    let expected = Some(Metadata {
        comment: Some(line_comment("test".to_string())),
        ..Metadata::default()
    });
    assert_eq!(result, expected);
}

#[test]
fn suggest_metadata_at_field_access() {
    let _ = env_logger::try_init();

    let text = r#"
let module = {
        /// test
        abc = 1,
        abb = 2
    }
module.ab
"#;
    let result = suggest_metadata(text, BytePos::from(81), "abc");

    let expected = Some(Metadata {
        comment: Some(line_comment("test".to_string())),
        ..Metadata::default()
    });
    assert_eq!(result, expected);
}

#[test]
fn find_all_symbols_test() {
    let _ = env_logger::try_init();

    let text = r#"
let test = 1
let dummy =
    let test = 3
    test
test #Int+ test #Int+ dummy
"#;
    let result = find_all_symbols(text, 6.into());

    assert_eq!(
        result,
        Ok((
            "test".to_string(),
            vec![
                Span::new(loc(text, 1, 4), loc(text, 1, 8)),
                Span::new(loc(text, 5, 0), loc(text, 5, 4)),
                Span::new(loc(text, 5, 11), loc(text, 5, 15)),
            ]
        ))
    );
}

#[test]
fn all_symbols_test() {
    let _ = env_logger::try_init();

    let text = r#"
let test = 1
let dummy =
    let test = 3
    test
type Abc a = a Int
// Unpacked values are not counted because they probably originated in another module
let { x, y } = { x = 1, y = 2 }
1
"#;

    let (expr, result) = support::typecheck_expr(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let symbols = completion::all_symbols(expr.span, &expr);

    assert_eq!(symbols.len(), 4);
}
