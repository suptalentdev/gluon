#[macro_use]
extern crate collect_mac;
extern crate env_logger;
#[macro_use]
extern crate pretty_assertions;

extern crate gluon_base as base;
extern crate gluon_check as check;
extern crate gluon_format as format;
extern crate gluon_parser as parser;

use base::ast::{self, SpannedExpr, Visitor};
use base::types::{Field, Type};
use base::symbol::Symbol;

#[macro_use]
#[allow(unused_macros)]
mod support;

#[test]
fn single_implicit_arg() {
    let _ = ::env_logger::init();
    let text = r#"

let f x y: [Int] -> Int -> Int = x
/// @implicit
let i = 123
f 42
"#;
    let result = support::typecheck(text);

    assert_eq!(result, Ok(Type::int()));
}

#[test]
fn multiple_implicit_args() {
    let _ = ::env_logger::init();
    let text = r#"

let f x y z w: [Int] -> [String] -> String -> Int -> Int = x
/// @implicit
let i = 123
/// @implicit
let x = "abc"
f x 42
"#;
    let result = support::typecheck(text);

    assert_eq!(result, Ok(Type::int()));
}

#[test]
fn just_a_implicit_arg() {
    let _ = ::env_logger::init();
    let text = r#"

let f x: [Int] -> Int = x
/// @implicit
let i = 123
f
"#;
    let result = support::typecheck(text);

    assert_eq!(result, Ok(Type::int()));
}

#[test]
fn function_implicit_arg() {
    let _ = ::env_logger::init();
    let text = r#"

let f eq l r: [a -> a -> Bool] -> a -> a -> Bool = eq l r
/// @implicit
let eq_int l r : Int -> Int -> Bool = True
/// @implicit
let eq_string l r : String -> String -> Bool = True
f 1 2
f "" ""
()
"#;
    let result = support::typecheck(text);

    assert_req!(result, Ok(Type::unit()));
}

#[test]
fn infix_implicit_arg() {
    let _ = ::env_logger::init();
    let text = r#"

let (==) eq l r: [a -> a -> Bool] -> a -> a -> Bool = eq l r
/// @implicit
let eq_int l r : Int -> Int -> Bool = True
/// @implicit
let eq_string l r : String -> String -> Bool = True
"" == ""
"#;
    let (mut expr, _result) = support::typecheck_expr(text);

    loop {
        match expr.value {
            ast::Expr::LetBindings(_, body) => {
                expr = *body;
            }
            _ => match expr.value {
                ast::Expr::App { ref args, .. } if args.len() == 3 => {
                    break;
                }
                _ => assert!(false),
            },
        }
    }
}

#[test]
fn implicit_from_record_field() {
    let _ = ::env_logger::init();
    let text = r#"

let f eq l r: [a -> a -> Bool] -> a -> a -> Bool = eq l r
/// @implicit
let eq_int l r : Int -> Int -> Bool = True
let eq_string =
    /// @implicit
    let eq l r : String -> String -> Bool = True
    { eq }
f 1 2
f "" ""
()
"#;
    let result = support::typecheck(text);

    assert_req!(result, Ok(Type::unit()));
}

#[test]
fn implicit_on_type() {
    let _ = ::env_logger::init();
    let text = r#"
/// @implicit
type Test = | Test
let f x y: [a] -> a -> a = x
let i = Test
f Test
"#;
    let result = support::typecheck(text);

    let test = support::alias(
        "Test",
        &[],
        Type::variant(vec![
            Field::new(support::intern("Test"), support::typ("Test")),
        ]),
    );
    assert_eq!(result, Ok(test));
}

#[test]
fn forward_implicit_parameter() {
    let _ = ::env_logger::init();
    let text = r#"
/// @implicit
type Test a = | Test a
let f x : [Test a] -> Test a = x
let g x y : [Test a] -> a -> Test a = f
let i = Test 1
g 2
()
"#;
    let (expr, result) = support::typecheck_expr(text);

    assert_eq!(result, Ok(Type::unit()));

    struct Visitor {
        text: &'static str,
        done: bool,
    }
    impl<'a> base::ast::Visitor<'a> for Visitor {
        type Ident = Symbol;

        fn visit_expr(&mut self, expr: &'a SpannedExpr<Symbol>) {
            match expr.value {
                ast::Expr::LetBindings(ref bindings, _) => {
                    if let ast::Pattern::Ident(ref id) = bindings[0].name.value {
                        if id.name.definition_name() == "g" {
                            assert_eq!(
                                "f x",
                                format::pretty_expr(self.text, &bindings[0].expr).trim()
                            );
                            self.done = true;
                        }
                    }
                }
                _ => (),
            }
            base::ast::walk_expr(self, expr)
        }
    }
    let mut visitor = Visitor { text, done: false };
    visitor.visit_expr(&expr);
    assert!(visitor.done);
}

#[test]
fn implicit_as_function_argument() {
    let _ = ::env_logger::init();
    let text = r#"
let (==) eq l r: [a -> a -> Bool] -> a -> a -> Bool = eq l r
/// @implicit
let eq_int l r : Int -> Int -> Bool = True
/// @implicit
let eq_string l r : String -> String -> Bool = True

let f eq l r : (a -> a -> Bool) -> a -> a -> Bool = eq l r
f (==) 1 2
"#;
    let (mut expr, result) = support::typecheck_expr(text);

    assert!(result.is_ok(), "{}", result.unwrap_err());

    loop {
        match expr.value {
            ast::Expr::LetBindings(_, body) => {
                expr = *body;
            }
            _ => match expr.value {
                ast::Expr::App { ref args, .. } if args.len() == 3 => {
                    assert_eq!(args.len(), 3);
                    match args[0].value {
                        ast::Expr::App {
                            args: ref args_eq, ..
                        } => {
                            assert_eq!(args_eq.len(), 1);
                        }
                        _ => panic!(),
                    }
                    break;
                }
                _ => assert!(false),
            },
        }
    }
}

#[test]
fn applicative_resolve_implicit() {
    let _ = ::env_logger::init();
    let text = r#"
/// @implicit
type Functor f = {
    map : forall a b . (a -> b) -> f a -> f b
}

/// @implicit
type Applicative (f : Type -> Type) = {
    functor : Functor f,
    apply : forall a b . f (a -> b) -> f a -> f b,
}

let (<*>) app : [Applicative f] -> f (a -> b) -> f a -> f b = app.apply
let (<*) app l r : [Applicative f] -> f a -> f b -> f a = app.functor.map (\x _ -> x) l <*> r
()
"#;
    let result = support::typecheck(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());
}

#[test]
fn select_functor_from_applicative() {
    let _ = ::env_logger::init();
    let text = r#"
/// @implicit
type Functor f = {
    map : forall a b . (a -> b) -> f a -> f b
}

/// @implicit
type Applicative (f : Type -> Type) = {
    functor : Functor f,
    apply : forall a b . f (a -> b) -> f a -> f b,
}

let map functor : [Functor f] -> (a -> b) -> f a -> f b = functor.map

let test app f xs: [Applicative f] -> (a -> b) -> f a -> f b =
    map f xs
()
"#;
    let result = support::typecheck(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());
}

#[test]
fn wrap_call_selection() {
    let _ = ::env_logger::init();
    let text = r#"

/// @implicit
type Applicative (f : Type -> Type) = {
    wrap : forall a . a -> f a,
}

let wrap app : [Applicative f] -> a -> f a = app.wrap

type Test a = | Test a

let applicative : Applicative Test = {
    wrap = Test
}

let x: a -> Test Int = \_ -> wrap 123
()
"#;
    let result = support::typecheck(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());
}

#[test]
fn unknown_implicit_arg_type() {
    let _ = ::env_logger::init();
    let text = r#"

/// @implicit
type Applicative (f : Type -> Type) = {
    wrap : forall a . a -> f a,
}

let wrap app : [Applicative f] -> a -> f a = app.wrap

type Test a = | Test a

let applicative : Applicative Test = {
    wrap = Test
}

\_ -> wrap 123
()
"#;
    let (expr, result) = support::typecheck_expr(text);

    struct Visitor {
        done: bool,
    }
    impl<'a> base::ast::Visitor<'a> for Visitor {
        type Ident = Symbol;

        fn visit_expr(&mut self, expr: &'a SpannedExpr<Symbol>) {
            match expr.value {
                ast::Expr::Lambda(ref lambda) => {
                    assert_eq!(lambda.args.len(), 2);
                    assert_eq!(
                        lambda.id.typ.to_string(),
                        "forall a a0 . [test.Applicative a0] -> a -> a0 Int"
                    );
                    self.done = true;
                }
                _ => (),
            }
            base::ast::walk_expr(self, expr)
        }
    }
    let mut visitor = Visitor { done: false };
    visitor.visit_expr(&expr);
    assert!(visitor.done);
    assert!(result.is_ok(), "{}", result.unwrap_err());
}

#[test]
fn dont_insert_extra_implicit_arg_type() {
    let _ = ::env_logger::init();
    let text = r#"

/// @implicit
type Applicative (f : Type -> Type) = {
    wrap : forall a . a -> f a,
}

let wrap app : [Applicative f] -> a -> f a = app.wrap

type Test a = | Test a

let applicative : Applicative Test = {
    wrap = Test
}

wrap
"#;
    let result = support::typecheck(text);
    assert_req!(
        result.map(|typ| typ.to_string()),
        Ok("forall a f . [test.Applicative f] -> a -> f a")
    );
}
