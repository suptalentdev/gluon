#[macro_use]
extern crate collect_mac;
extern crate env_logger;
extern crate gluon_base as base;
extern crate gluon_parser as parser;
#[macro_use]
extern crate pretty_assertions;

#[macro_use]
mod support;

use crate::support::*;

use crate::base::metadata::*;
use crate::base::pos::{self, BytePos, Span, Spanned};
use crate::base::types::{Alias, Field, Type};
use crate::base::{ast::*, mk_ast_arena};

use crate::parser::ReplLine;

test_parse! {
    dangling_in,
r#"
let x = 1
in

let y = 2
y
"#,
    |arena| let_(arena, "x", int(1), let_(arena, "y", int(2), id("y")))
}

test_parse! {
    expression1,
    "2 * 3 + 4",
    |arena| binop(arena, binop(arena, int(2), "*", int(3)), "+", int(4))
}

test_parse! {
    expression2,
    r#"\x y -> x + y"#,
    |arena| lambda(arena,
        "",
        vec![intern("x"), intern("y")],
        binop(arena, id("x"), "+", id("y")),
    )
}

test_parse! {
    expression3,
    r#"type Test = Int in 0"#,
    |arena| type_decl(arena, intern("Test"), vec![], typ("Int"), int(0))
}

test_parse! {
    application,
    "let f = \\x y -> x + y in f 1 2",
    |arena| let_(arena,
        "f",
        lambda(arena,
            "",
            vec![intern("x"), intern("y")],
            binop(arena, id("x"), "+", id("y")),
        ),
        app(arena, id("f"), vec![int(1), int(2)]),
    )
}

test_parse! {
if_else_test,
    "if True then 1 else 0",
    |arena| if_else(arena, id("True"), int(1), int(0))
}

#[test]
fn let_type_decl() {
    let _ = ::env_logger::try_init();
    let e = parse_clear_span!("let f: Int = \\x y -> x + y in f 1 2");
    match &e.expr().value {
        Expr::LetBindings(bind, _) => assert_eq!(bind[0].typ, Some(typ("Int"))),
        _ => assert!(false),
    }
}

test_parse! {
    let_args,
    "let f x y = y in f 2",
    |arena| let_a(arena, "f", &["x", "y"], id("y"), app(arena, id("f"), vec![int(2)]))
}

test_parse! {
type_decl_record,
    "type Test = { x: Int, y: {} } in 1",
    |arena| {
        let record = Type::record(
            Vec::new(),
            vec![
                Field::new(intern("x"), typ("Int")),
                Field::new(intern("y"), Type::record(vec![], vec![])),
            ],
        );
        type_decl(arena, intern("Test"), vec![], record, int(1))
    }
}

test_parse! {
    type_mutually_recursive,
    r#"
    rec
    /// Test
    type Test = | Test Int
    #[a]
    type Test2 = { x: Int, y: {} }
    in 1"#,
    |arena| {
        let test = Type::variant(vec![Field::ctor(intern("Test"), vec![typ("Int")])]);
        let test2 = Type::record(
            Vec::new(),
            vec![
                Field::new(intern("x"), typ("Int")),
                Field::new(intern("y"), Type::record(vec![], vec![])),
            ],
        );
        let binds = vec![
            TypeBinding {
                metadata: line_comment("Test"),
                name: no_loc(intern("Test")),
                alias: alias(intern("Test"), Vec::new(), test),
                finalized_alias: None,
            },
            TypeBinding {
                metadata: Metadata {
                    attributes: vec![Attribute {
                        name: "a".into(),
                        arguments: None,
                    }],
                    ..Metadata::default()
                },
                name: no_loc(intern("Test2")),
                alias: alias(intern("Test2"), Vec::new(), test2),
                finalized_alias: None,
            },
        ];
        type_decls(arena, binds, int(1))
    }
}

test_parse! {
type_decl_projection,
    "type Test = x.y.Z in 1",
    |arena| {
        let record = Type::projection(["x", "y", "Z"].iter().map(|s| intern(s)).collect());
        type_decl(arena, intern("Test"), vec![], record, int(1))
    }
}

#[test]
fn tuple_type() {
    let _ = ::env_logger::try_init();

    let expr = r#"
        let _: (Int, String, Option Int) = (1, "", None)
        1"#;
    parse_new!(expr);
}

test_parse! {
    field_access_test,
    "{ x = 1 }.x",
    |arena| field_access(arena, record(arena, vec![(intern("x"), Some(int(1)))]), "x")
}

test_parse! {
    builtin_op,
    "x #Int+ 1",
    |arena| binop(arena, id("x"), "#Int+", int(1))
}

test_parse! {
    op_identifier,
    "let (==) = \\x y -> x #Int== y in (==) 1 2",
    |arena| {
        let_(arena,
            "==",
            lambda(arena,
                "",
                vec![intern("x"), intern("y")],
                binop(arena, id("x"), "#Int==", id("y")),
            ),
            app(arena, id("=="), vec![int(1), int(2)]),
        )
    }
}

test_parse! {
    variant_type,
    "type Option a = | None | Some a in Some 1",
    |arena| type_decl(arena,
            intern("Option"),
            vec![generic("a")],
            Type::variant(vec![
                Field::ctor(intern("None"), vec![]),
                Field::ctor(intern("Some"), vec![typ("a")]),
            ]),
            app(arena, id("Some"), vec![int(1)]),
        )
}

test_parse! {
    case_expr,
    r#"
    match None with
    | Some x -> x
    | None -> 0"#,
    |arena| case(arena,
            id("None"),
            vec![
                (
                    Pattern::Constructor(
                        TypedIdent::new(intern("Some")),
                        vec![no_loc(Pattern::Ident(TypedIdent::new(intern("x"))))],
                    ),
                    id("x"),
                ),
                (
                    Pattern::Constructor(TypedIdent::new(intern("None")), vec![]),
                    int(0),
                ),
            ],
        )
}

test_parse! {
    array_expr,
    "[1, a]",
    |arena| array(arena, vec![int(1), id("a")])
}

test_parse! {
    operator_expr,
    "test + 1 * 23 #Int- test",
    |arena| binop(arena,
            binop(arena, id("test"), "+", binop(arena, int(1), "*", int(23))),
            "#Int-",
            id("test"),
        )
}

test_parse! {
    record_trailing_comma,
    "{ y, x = z,}",
    |arena| record(arena, vec![("y".into(), None), ("x".into(), Some(id("z")))])
}

test_parse! {
    array_trailing_comma,
    "[y, 1, 2,]",
    |arena| array(arena, vec![id("y"), int(1), int(2)])
}

test_parse! {
    record_pattern,
    "match x with | { y, x = z } -> z",
    |arena| {
        let pattern = Pattern::Record {
            typ: Type::hole(),
            types: Vec::new(),
            fields: vec![
                PatternField {
                    name: no_loc(intern("y")),
                    value: None,
                },
                PatternField {
                    name: no_loc(intern("x")),
                    value: Some(no_loc(Pattern::Ident(TypedIdent::new(intern("z"))))),
                },
            ],
            implicit_import: None,
        };
        case(arena, id("x"), vec![(pattern, id("z"))])
    }
}

test_parse! {
    let_pattern,
    "let {x, y} = test in x",
    |arena| no_loc(Expr::let_binding(
            ValueBinding {
                metadata: Metadata::default(),
                name: no_loc(Pattern::Record {
                    typ: Type::hole(),
                    types: Vec::new(),
                    fields: vec![
                        PatternField {
                            name: no_loc(intern("x")),
                            value: None,
                        },
                        PatternField {
                            name: no_loc(intern("y")),
                            value: None,
                        },
                    ],
                    implicit_import: None,
                }),
                typ: None,
                resolved_type: Type::hole(),
                args: vec![],
                expr: id("test"),
            },
            arena.alloc(id("x")),
        ))
}

test_parse! {
    nested_pattern,
    "match x with | { y = Some x } -> z",
    |arena| {
        let nested = no_loc(Pattern::Constructor(
            TypedIdent::new(intern("Some")),
            vec![no_loc(Pattern::Ident(TypedIdent::new(intern("x"))))],
        ));
        let pattern = Pattern::Record {
            typ: Type::hole(),
            types: Vec::new(),
            fields: vec![PatternField {
                name: no_loc(intern("y")),
                value: Some(nested),
            }],
            implicit_import: None,
        };
        case(arena, id("x"), vec![(pattern, id("z"))])
    }
}

test_parse! {
    nested_pattern_parens,
    "match x with | (Some (Some z)) -> z",
    |arena| {
        let inner_pattern = no_loc(Pattern::Constructor(
            TypedIdent::new(intern("Some")),
            vec![no_loc(Pattern::Ident(TypedIdent::new(intern("z"))))],
        ));
        let pattern = Pattern::Constructor(TypedIdent::new(intern("Some")), vec![inner_pattern]);
        case(arena, id("x"), vec![(pattern, id("z"))])
    }
}

#[test]
fn span_identifier() {
    let _ = ::env_logger::try_init();

    let e = parse_zero_index!("test");
    assert_eq!(e.expr().span, Span::new(BytePos::from(0), BytePos::from(4)));
}

#[test]
fn span_integer() {
    let _ = ::env_logger::try_init();

    let e = parse_zero_index!("1234");
    assert_eq!(e.expr().span, Span::new(BytePos::from(0), BytePos::from(4)));
}

#[test]
fn span_string_literal() {
    let _ = ::env_logger::try_init();

    let e = parse_zero_index!(r#" "test" "#);
    assert_eq!(e.expr().span, Span::new(BytePos::from(1), BytePos::from(7)));
}

#[test]
fn span_app() {
    let _ = ::env_logger::try_init();

    let e = parse_zero_index!(r#" f 123 "asd""#);
    assert_eq!(
        e.expr().span,
        Span::new(BytePos::from(1), BytePos::from(12))
    );
}

#[test]
fn span_match() {
    let _ = ::env_logger::try_init();

    let e = parse_zero_index!(
        r#"
match False with
    | True -> "asd"
    | False -> ""
"#
    );
    assert_eq!(
        e.expr().span,
        Span::new(BytePos::from(1), BytePos::from(55))
    );
}

#[test]
fn span_if_else() {
    let _ = ::env_logger::try_init();

    let e = parse_zero_index!(
        r#"
if True then
    1
else
    123.45
"#
    );
    assert_eq!(
        e.expr().span,
        Span::new(BytePos::from(1), BytePos::from(35))
    );
}

#[test]
fn span_byte() {
    let _ = ::env_logger::try_init();

    let e = parse_zero_index!(r#"124b"#);
    assert_eq!(e.expr().span, Span::new(BytePos::from(0), BytePos::from(4)));
}

#[test]
fn span_field_access() {
    let _ = ::env_logger::try_init();
    let expr = parse_zero_index!("record.x");
    assert_eq!(
        expr.expr().span,
        Span::new(BytePos::from(0), BytePos::from(8))
    );
    match expr.expr().value {
        Expr::Projection(ref e, _, _) => {
            assert_eq!(e.span, Span::new(BytePos::from(0), BytePos::from(6)));
        }
        _ => panic!(),
    }
}

#[test]
fn comment_on_let() {
    let _ = ::env_logger::try_init();
    let text = r#"
/// The identity function
let id x = x
id
"#;
    let e = parse_clear_span!(text);
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        no_loc(Expr::LetBindings(
            ValueBindings::Plain(Box::new(ValueBinding {
                metadata: Metadata {
                    comment: Some(Comment {
                        typ: CommentType::Line,
                        content: "The identity function".into(),
                    }),
                    ..Metadata::default()
                },
                name: no_loc(Pattern::Ident(TypedIdent::new(intern("id")))),
                typ: None,
                resolved_type: Type::hole(),
                args: vec![Argument::explicit(no_loc(TypedIdent::new(intern("x"))))],
                expr: id("x"),
            })),
            arena.alloc(id("id")),
        ),)
    );
}

#[test]
fn comment_on_rec_let() {
    let _ = ::env_logger::try_init();
    let text = r#"
rec
let id x = x
/// The identity function
let id2 y = y
id
"#;
    let e = parse_clear_span!(text);
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        no_loc(Expr::LetBindings(
            ValueBindings::Recursive(vec![
                ValueBinding {
                    metadata: Metadata::default(),
                    name: no_loc(Pattern::Ident(TypedIdent::new(intern("id")))),
                    typ: None,
                    resolved_type: Type::hole(),
                    args: vec![Argument::explicit(no_loc(TypedIdent::new(intern("x"))))],
                    expr: id("x"),
                },
                ValueBinding {
                    metadata: Metadata {
                        comment: Some(Comment {
                            typ: CommentType::Line,
                            content: "The identity function".into(),
                        }),
                        ..Metadata::default()
                    },
                    name: no_loc(Pattern::Ident(TypedIdent::new(intern("id2")))),
                    typ: None,
                    resolved_type: Type::hole(),
                    args: vec![Argument::explicit(no_loc(TypedIdent::new(intern("y"))))],
                    expr: id("y"),
                },
            ]),
            arena.alloc(id("id")),
        ))
    );
}

#[test]
fn comment_on_type() {
    let _ = ::env_logger::try_init();
    let text = r#"
/** Test type */
type Test = Int
id
"#;
    let e = parse_clear_span!(text);
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        type_decls(
            &arena,
            vec![TypeBinding {
                metadata: Metadata {
                    comment: Some(Comment {
                        typ: CommentType::Block,
                        content: "Test type".into(),
                    }),
                    ..Metadata::default()
                },
                name: no_loc(intern("Test")),
                alias: alias(intern("Test"), Vec::new(), typ("Int")),
                finalized_alias: None,
            }],
            id("id"),
        )
    );
}

#[test]
fn comment_after_integer() {
    let _ = ::env_logger::try_init();
    let text = r#"
let x = 1

/** Test type */
type Test = Int
id
"#;
    let e = parse_clear_span!(text);
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        let_a(
            &arena,
            "x",
            &[],
            int(1),
            type_decls(
                &arena,
                vec![TypeBinding {
                    metadata: Metadata {
                        comment: Some(Comment {
                            typ: CommentType::Block,
                            content: "Test type".into(),
                        }),
                        ..Metadata::default()
                    },
                    name: no_loc(intern("Test")),
                    alias: alias(intern("Test"), Vec::new(), typ("Int")),
                    finalized_alias: None,
                }],
                id("id"),
            ),
        )
    );
}

#[test]
fn merge_line_comments() {
    let _ = ::env_logger::try_init();
    let text = r#"
/// Merge
/// consecutive
/// line comments.
type Test = Int
id
"#;
    let e = parse_clear_span!(text);
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        type_decls(
            &arena,
            vec![TypeBinding {
                metadata: Metadata {
                    comment: Some(Comment {
                        typ: CommentType::Line,
                        content: "Merge\nconsecutive\nline comments.".into(),
                    }),
                    ..Metadata::default()
                },
                name: no_loc(intern("Test")),
                alias: alias(intern("Test"), Vec::new(), typ("Int")),
                finalized_alias: None,
            }],
            id("id"),
        )
    );
}

#[test]
fn partial_field_access_simple() {
    let _ = ::env_logger::try_init();
    let text = r#"test."#;
    let e = parse(text);
    assert!(e.is_err());
    let e = clear_span(e.unwrap_err().0.unwrap());
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        no_loc(Expr::Projection(
            arena.alloc(id("test")),
            intern(""),
            Type::hole()
        ),)
    );
}

#[test]
fn partial_field_access_in_block() {
    let _ = ::env_logger::try_init();
    let text = r#"
test.
test
"#;
    let e = parse(text);
    assert!(e.is_err());
    let e = clear_span(e.unwrap_err().0.unwrap());
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        no_loc(Expr::Block(arena.alloc_extend(vec![
            Spanned {
                span: Span::new(BytePos::from(0), BytePos::from(0)),
                value: Expr::Projection(arena.alloc(id("test")), intern(""), Type::hole()),
            },
            id("test"),
        ])))
    );
}

#[test]
fn function_operator_application() {
    let _ = ::env_logger::try_init();
    let text = r#"
let x: ((->) Int Int) = x
x
"#;
    let e = parse_clear_span!(text);
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        no_loc(Expr::LetBindings(
            ValueBindings::Plain(Box::new(ValueBinding {
                metadata: Metadata::default(),
                name: no_loc(Pattern::Ident(TypedIdent::new(intern("x")))),
                typ: Some(Type::app(typ("->"), collect![typ("Int"), typ("Int")])),
                resolved_type: Type::hole(),
                args: vec![],
                expr: id("x"),
            })),
            arena.alloc(id("x")),
        ),)
    );
}

#[test]
fn quote_in_identifier() {
    let _ = ::env_logger::try_init();
    let e = parse_clear_span!("let f' = \\x y -> x + y in f' 1 2");
    mk_ast_arena!(arena);
    let a = let_(
        &arena,
        "f'",
        lambda(
            &arena,
            "",
            vec![intern("x"), intern("y")],
            binop(&arena, id("x"), "+", id("y")),
        ),
        app(&arena, id("f'"), vec![int(1), int(2)]),
    );
    assert_eq!(*e.expr(), a);
}

// Test that this is `rec let x = 1 in {{ a; b }}` let not `{{ (let x = 1 in a) ; b }}`
#[test]
fn block_open_after_let_in() {
    let _ = ::env_logger::try_init();
    let text = r#"
        let x = 1
        a
        b
        "#;
    let e = parse_zero_index!(text);
    match e.expr().value {
        Expr::LetBindings(..) => (),
        _ => panic!("{:?}", e),
    }
}

#[test]
fn block_open_after_explicit_let_in() {
    let _ = ::env_logger::try_init();
    let text = r#"
        let x = 1
        in
        a
        b
        "#;
    let e = parse_zero_index!(text);
    match e.expr().value {
        Expr::LetBindings(..) => (),
        _ => panic!("{:?}", e),
    }
}

#[test]
fn record_type_field() {
    let _ = ::env_logger::try_init();
    let text = r"{ Test, x }";
    let e = parse_clear_span!(text);
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        record_a(
            &arena,
            vec![("Test".into(), None)],
            vec![("x".into(), None)]
        )
    )
}

#[test]
fn parse_macro() {
    let _ = ::env_logger::try_init();
    let text = r#" import! "#;
    let e = parse_clear_span!(text);
    assert_eq!(*e.expr(), id("import!"));
}

#[test]
fn doc_comment_on_record_field() {
    let _ = ::env_logger::try_init();
    let text = r"{ /** test*/ Test,
    /// x binding
    x = 1 }";
    let e = parse_clear_span!(text);
    assert_eq!(
        *e.expr(),
        no_loc(Expr::Record {
            typ: Type::hole(),
            types: vec![ExprField {
                metadata: Metadata {
                    comment: Some(Comment {
                        typ: CommentType::Block,
                        content: "test".into(),
                    }),
                    ..Metadata::default()
                },
                name: no_loc("Test".into()),
                value: None,
            }],
            exprs: vec![ExprField {
                metadata: Metadata {
                    comment: Some(Comment {
                        typ: CommentType::Line,
                        content: "x binding".into(),
                    }),
                    ..Metadata::default()
                },
                name: no_loc("x".into()),
                value: Some(int(1)),
            }],
            base: None,
        })
    )
}

#[test]
fn shebang_at_top_is_ignored() {
    let _ = ::env_logger::try_init();
    let text = r"#!/bin/gluon
{ Test, x }";
    let e = parse_clear_span!(text);
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        record_a(
            &arena,
            vec![("Test".into(), None)],
            vec![("x".into(), None)]
        )
    )
}

#[test]
fn do_in_parens() {
    let _ = ::env_logger::try_init();
    let text = r"
        scope_state (
            seq add_args
            eval_exprs
        )
    ";
    parse_clear_span!(text);
}

#[test]
fn parse_repl_line() {
    let _ = ::env_logger::try_init();

    let mut module = MockEnv::new();

    let line = "let x = test";
    mk_ast_arena!(arena);
    match parser::parse_partial_repl_line(&arena, &mut module, line) {
        Ok(x) => assert_eq!(
            x,
            Some(ReplLine::Let(ValueBinding {
                metadata: Metadata::default(),
                name: pos::spanned2(
                    // Add one to each position since codespan return 1-indexed positions
                    5.into(),
                    6.into(),
                    Pattern::Ident(TypedIdent::new(intern("x")))
                ),
                typ: None,
                resolved_type: Type::hole(),
                args: Vec::new(),
                expr: pos::spanned2(
                    9.into(),
                    13.into(),
                    Expr::Ident(TypedIdent::new(intern("test")))
                ),
            }))
        ),
        Err((_, err)) => panic!("{}", err),
    }
}

#[test]
fn alias_in_record_type() {
    let _ = ::env_logger::try_init();

    let text = r#"
        type Test = { MyInt }
        1
        "#;
    let e = parse_clear_span!(text);
    mk_ast_arena!(arena);
    assert_eq!(
        *e.expr(),
        no_loc(Expr::TypeBindings(
            vec![TypeBinding {
                metadata: Metadata::default(),
                name: no_loc(intern("Test")),
                alias: alias(
                    intern("Test"),
                    Vec::new(),
                    Type::record(
                        vec![Field {
                            name: intern("MyInt"),
                            typ: Alias::new(intern("MyInt"), Vec::new(), Type::hole()),
                        }],
                        vec![],
                    ),
                ),
                finalized_alias: None,
            }],
            arena.alloc(int(1))
        ),)
    )
}

#[test]
fn let_then_doc_comment() {
    let _ = ::env_logger::try_init();
    let text = r"
let map2 = 1

/// Maps over three actions
let map3 = 2
()
    ";
    parse_clear_span!(text);
}

#[test]
fn rec_let_indentation() {
    let _ = ::env_logger::try_init();
    let text = r#"
rec let id x =
    let y = x
    y
id
"#;
    parse_clear_span!(text);
}

#[test]
fn rec_let_with_doc_comment() {
    let _ = ::env_logger::try_init();
    let text = r#"
let x = { }

/// a
rec let id x =
    let y = x
    y
id
"#;
    parse_clear_span!(text);
}

#[test]
fn rec_let_rec_let() {
    let _ = ::env_logger::try_init();
    let text = r#"
rec let x = 0

rec let y = 2

1
"#;
    parse_clear_span!(text);
}

#[test]
fn rec_let_doc_rec_let() {
    let _ = ::env_logger::try_init();
    let text = r#"
rec let x = 0

/// y
rec let y = 2

1
"#;
    parse_clear_span!(text);
}

#[test]
fn gadt() {
    let _ = ::env_logger::try_init();
    let text = r#"
type Expr a =
    | Int : Int -> Expr Int
    | Add : Expr Int -> Expr Int -> Expr Int
    | If : Expr Bool -> Expr a -> Expr a -> Expr a


1
"#;
    parse_clear_span!(text);
}
