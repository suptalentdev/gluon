extern crate base;
extern crate parser;
extern crate env_logger;
#[macro_use]
extern crate log;

use base::ast::*;
use base::ast;
use base::types;
use base::types::{Type, TypeConstructor, Generic, Alias, Field, Kind};
use parser::{parse_string, Error};

pub fn intern(s: &str) -> String {
    String::from(s)
}

type PExpr = LExpr<String>;

fn binop(l: PExpr, s: &str, r: PExpr) -> PExpr {
    no_loc(Expr::BinOp(Box::new(l), intern(s), Box::new(r)))
}
fn int(i: i64) -> PExpr {
    no_loc(Expr::Literal(LiteralEnum::Integer(i)))
}
fn let_(s: &str, e: PExpr, b: PExpr) -> PExpr {
    let_a(s, &[], e, b)
}
fn let_a(s: &str, args: &[&str], e: PExpr, b: PExpr) -> PExpr {
    no_loc(Expr::Let(vec![Binding {
                              name: no_loc(Pattern::Identifier(intern(s))),
                              typ: None,
                              arguments: args.iter().map(|i| intern(i)).collect(),
                              expression: e,
                          }],
                     Box::new(b)))
}
fn id(s: &str) -> PExpr {
    no_loc(Expr::Identifier(intern(s)))
}
fn field(s: &str, typ: ASTType<String>) -> Field<String> {
    Field {
        name: intern(s),
        typ: typ,
    }
}
fn typ(s: &str) -> ASTType<String> {
    assert!(s.len() != 0);
    let is_var = s.chars().next().unwrap().is_lowercase();
    match s.parse() {
        Ok(b) => Type::builtin(b),
        Err(()) if is_var => generic(s),
        Err(()) => Type::data(TypeConstructor::Data(intern(s)), Vec::new()),
    }
}
fn typ_a(s: &str, args: Vec<ASTType<String>>) -> ASTType<String> {
    assert!(s.len() != 0);
    ASTType::new(types::type_con(intern(s), args))
}
fn generic(s: &str) -> ASTType<String> {
    Type::generic(Generic {
        kind: Kind::variable(0),
        id: intern(s),
    })
}
fn call(e: PExpr, args: Vec<PExpr>) -> PExpr {
    no_loc(Expr::Call(Box::new(e), args))
}
fn if_else(p: PExpr, if_true: PExpr, if_false: PExpr) -> PExpr {
    no_loc(Expr::IfElse(Box::new(p), Box::new(if_true), Some(Box::new(if_false))))
}
fn case(e: PExpr, alts: Vec<(Pattern<String>, PExpr)>) -> PExpr {
    no_loc(Expr::Match(Box::new(e),
                       alts.into_iter()
                           .map(|(p, e)| {
                               Alternative {
                                   pattern: no_loc(p),
                                   expression: e,
                               }
                           })
                           .collect()))
}
fn lambda(name: &str, args: Vec<String>, body: PExpr) -> PExpr {
    no_loc(Expr::Lambda(Lambda {
        id: intern(name),
        free_vars: Vec::new(),
        arguments: args,
        body: Box::new(body),
    }))
}

fn type_decl(name: ASTType<String>, typ: ASTType<String>, body: PExpr) -> PExpr {
    type_decls(vec![TypeBinding {
                        name: name,
                        typ: typ,
                    }],
               body)
}

fn type_decls(binds: Vec<TypeBinding<String>>, body: PExpr) -> PExpr {
    no_loc(Expr::Type(binds, Box::new(body)))
}

fn bool(b: bool) -> PExpr {
    no_loc(Expr::Literal(LiteralEnum::Bool(b)))
}
fn record(fields: Vec<(String, Option<PExpr>)>) -> PExpr {
    record_a(Vec::new(), fields)
}
fn record_a(types: Vec<(String, Option<ASTType<String>>)>,
            fields: Vec<(String, Option<PExpr>)>)
            -> PExpr {
    no_loc(Expr::Record {
        typ: intern(""),
        types: types,
        exprs: fields,
    })
}
fn field_access(expr: PExpr, field: &str) -> PExpr {
    no_loc(Expr::FieldAccess(Box::new(expr), intern(field)))
}
fn array(fields: Vec<PExpr>) -> PExpr {
    no_loc(Expr::Array(Array {
        id: intern(""),
        expressions: fields,
    }))
}

fn parse(input: &str) -> Result<LExpr<String>, Error> {
    parse_string(None, &mut ast::EmptyEnv::new(), input)
}

fn parse_new(input: &str) -> LExpr<String> {
    parse(input).unwrap_or_else(|err| panic!("{:?}", err))
}

#[test]
fn dangling_in() {
    let _ = ::env_logger::init();
    // Check that the lexer does not insert an extra `in`
    let text = r#"
let x = 1
in

let y = 2
y
"#;
    let e = parse_new(text);
    assert_eq!(e, let_("x", int(1), let_("y", int(2), id("y"))));
}

#[test]
fn expression() {
    let _ = ::env_logger::init();
    let e = parse("2 * 3 + 4");
    assert_eq!(e, Ok(binop(binop(int(2), "*", int(3)), "+", int(4))));
    let e = parse(r#"\x y -> x + y"#);
    assert_eq!(e,
               Ok(lambda("",
                         vec![intern("x"), intern("y")],
                         binop(id("x"), "+", id("y")))));
    let e = parse(r#"type Test = Int in 0"#);
    assert_eq!(e, Ok(type_decl(typ("Test"), typ("Int"), int(0))));
}

#[test]
fn application() {
    let _ = ::env_logger::init();
    let e = parse_new("let f = \\x y -> x + y in f 1 2");
    let a = let_("f",
                 lambda("",
                        vec![intern("x"), intern("y")],
                        binop(id("x"), "+", id("y"))),
                 call(id("f"), vec![int(1), int(2)]));
    assert_eq!(e, a);
}

#[test]
fn if_else_test() {
    let _ = ::env_logger::init();
    let e = parse_new("if True then 1 else 0");
    let a = if_else(bool(true), int(1), int(0));
    assert_eq!(e, a);
}

#[test]
fn let_type_decl() {
    let _ = ::env_logger::init();
    let e = parse_new("let f: Int = \\x y -> x + y in f 1 2");
    match e.value {
        Expr::Let(bind, _) => assert_eq!(bind[0].typ, Some(typ("Int"))),
        _ => assert!(false),
    }
}
#[test]
fn let_args() {
    let _ = ::env_logger::init();
    let e = parse_new("let f x y = y in f 2");
    assert_eq!(e,
               let_a("f", &["x", "y"], id("y"), call(id("f"), vec![int(2)])));
}

#[test]
fn type_decl_record() {
    let _ = ::env_logger::init();
    let e = parse_new("type Test = { x: Int, y: {} } in 1");
    let record = Type::record(Vec::new(),
                              vec![field("x", typ("Int")),
                                   field("y", Type::record(vec![], vec![]))]);
    assert_eq!(e, type_decl(typ("Test"), record, int(1)));
}

#[test]
fn type_mutually_recursive() {
    let _ = ::env_logger::init();
    let e = parse_new("type Test = | Test Int and Test2 = { x: Int, y: {} } in 1");
    let test = Type::variants(vec![(intern("Test"),
                                    Type::function(vec![typ("Int")], typ("Test")))]);
    let test2 = Type::record(Vec::new(),
                             vec![Field {
                                      name: intern("x"),
                                      typ: typ("Int"),
                                  },
                                  Field {
                                      name: intern("y"),
                                      typ: Type::record(vec![], vec![]),
                                  }]);
    let binds = vec![
        TypeBinding { name: typ("Test"), typ: test },
        TypeBinding { name: typ("Test2"), typ: test2 },
        ];
    assert_eq!(e, type_decls(binds, int(1)));
}

#[test]
fn field_access_test() {
    let _ = ::env_logger::init();
    let e = parse_new("{ x = 1 }.x");
    assert_eq!(e,
               field_access(record(vec![(intern("x"), Some(int(1)))]), "x"));
}

#[test]
fn builtin_op() {
    let _ = ::env_logger::init();
    let e = parse_new("x #Int+ 1");
    assert_eq!(e, binop(id("x"), "#Int+", int(1)));
}

#[test]
fn op_identifier() {
    let _ = ::env_logger::init();
    let e = parse_new("let (==) = \\x y -> x #Int== y in (==) 1 2");
    assert_eq!(e,
               let_("==",
                    lambda("",
                           vec![intern("x"), intern("y")],
                           binop(id("x"), "#Int==", id("y"))),
                    call(id("=="), vec![int(1), int(2)])));
}
#[test]
fn variant_type() {
    let _ = ::env_logger::init();
    let e = parse_new("type Option a = | None | Some a in Some 1");
    let option = Type::data(TypeConstructor::Data(intern("Option")), vec![typ("a")]);
    let none = Type::function(vec![], option.clone());
    let some = Type::function(vec![typ("a")], option.clone());
    assert_eq!(e,
               type_decl(option,
                         Type::variants(vec![(intern("None"), none), (intern("Some"), some)]),
                         call(id("Some"), vec![int(1)])));
}
#[test]
fn case_expr() {
    let _ = ::env_logger::init();
    let text = r#"
case None of
    | Some x -> x
    | None -> 0"#;
    let e = parse(text);
    assert_eq!(e,
               Ok(case(id("None"),
                    vec![(Pattern::Constructor(intern("Some"), vec![intern("x")]),
                          id("x")),
                         (Pattern::Constructor(intern("None"), vec![]), int(0))])));
}
#[test]
fn array_expr() {
    let _ = ::env_logger::init();
    let e = parse_new("[1, a]");
    assert_eq!(e, array(vec![int(1), id("a")]));
}
#[test]
fn operator_expr() {
    let _ = ::env_logger::init();
    let e = parse_new("test + 1 * 23 #Int- test");
    assert_eq!(e,
               binop(binop(id("test"), "+", binop(int(1), "*", int(23))),
                     "#Int-",
                     id("test")));
}

#[test]
fn record_pattern() {
    let _ = ::env_logger::init();
    let e = parse_new("case x of | { y, x = z } -> z");
    let pattern = Pattern::Record {
        id: String::new(),
        types: Vec::new(),
        fields: vec![(intern("y"), None), (intern("x"), Some(intern("z")))],
    };
    assert_eq!(e, case(id("x"), vec![(pattern, id("z"))]));
}
#[test]
fn let_pattern() {
    let _ = ::env_logger::init();
    let e = parse_new("let {x, y} = test in x");
    assert_eq!(e,
               no_loc(Expr::Let(vec![Binding {
                                         name: no_loc(Pattern::Record {
                                             id: String::new(),
                                             types: Vec::new(),
                                             fields: vec![(intern("x"), None), (intern("y"), None)],
                                         }),
                                         typ: None,
                                         arguments: vec![],
                                         expression: id("test"),
                                     }],
                                Box::new(id("x")))));
}

#[test]
fn associated_record() {
    let _ = ::env_logger::init();
    let e = parse_new("type Test a = { Fn, x: a } in { Fn = Int -> Array Int, Test, x = 1 }");

    let test_type = Type::record(vec![Field {
                                          name: String::from("Fn"),
                                          typ: Alias {
                                              name: String::from("Fn"),
                                              args: vec![],
                                              typ: Some(typ("Fn")),
                                          },
                                      }],
                                 vec![Field {
                                          name: intern("x"),
                                          typ: typ("a"),
                                      }]);
    let fn_type = Type::function(vec![typ("Int")], typ_a("Array", vec![typ("Int")]));
    let record = record_a(vec![(intern("Fn"), Some(fn_type)), (intern("Test"), None)],
                          vec![(intern("x"), Some(int(1)))]);
    assert_eq!(e,
               type_decl(typ_a("Test", vec![typ("a")]), test_type, record));
}

#[test]
fn span() {
    let _ = ::env_logger::init();
    let loc = |r, c| {
        Location {
            column: c,
            row: r,
            absolute: 0,
        }
    };

    let e = parse_new("test");
    assert_eq!(e.span(&EmptyEnv::new()),
               Span {
                   start: loc(1, 1),
                   end: loc(1, 5),
               });

    let e = parse_new("1234");
    assert_eq!(e.span(&EmptyEnv::new()),
               Span {
                   start: loc(1, 1),
                   end: loc(1, 5),
               });

    let e = parse_new(r#" f 123 "asd" "#);
    assert_eq!(e.span(&EmptyEnv::new()),
               Span {
                   start: loc(1, 2),
                   end: loc(1, 13),
               });

    let e = parse_new(r#"
case False of
    | True -> "asd"
    | False -> ""
"#);
    assert_eq!(e.span(&EmptyEnv::new()),
               Span {
                   start: loc(2, 1),
                   end: loc(4, 18),
               });

    let e = parse_new(r#"
if True then
    1
else
    123.45
"#);
    assert_eq!(e.span(&EmptyEnv::new()),
               Span {
                   start: loc(2, 1),
                   end: loc(5, 11),
               });
}
