#[macro_use]
extern crate collect_mac;
extern crate env_logger;
#[macro_use]
extern crate pretty_assertions;

extern crate gluon_base as base;
extern crate gluon_check as check;
extern crate gluon_parser as parser;

use base::ast::{self, Typed};
use base::kind::Kind;
use base::types::{Alias, AliasData, ArcType, Field, Generic, Type};

use support::{alias, intern, typ, MockEnv};

#[macro_use]
#[allow(unused_macros)]
mod support;

macro_rules! assert_pass {
    ($e: expr) => {{
        if !$e.is_ok() {
            panic!("assert_pass: {}", $e.unwrap_err());
        }
    }};
}

/// Converts `Type::Alias` into the easy to construct `Type::Ident` variants to make the expected
/// types easier to write
fn make_ident_type(typ: ArcType) -> ArcType {
    use base::types::walk_move_type;
    walk_move_type(typ, &mut |typ| match **typ {
        Type::Alias(ref alias) => Some(Type::ident(alias.name.clone())),
        _ => None,
    })
}

#[test]
fn function_type_new() {
    let text = r"
\x -> x #Int+ 1
";
    let result = support::typecheck(text);

    assert_req!(result, Ok(Type::function(vec![typ("Int")], typ("Int"))));
}

#[test]
fn char_literal() {
    let _ = env_logger::init();

    let text = r"
'a'
";
    let result = support::typecheck(text);
    let expected = Ok(Type::char());

    assert_eq!(result, expected);
}

#[test]
fn byte_literal() {
    let _ = env_logger::init();

    let text = r"
1b
";
    let result = support::typecheck(text);
    let expected = Ok(Type::byte());

    assert_eq!(result, expected);
}

#[test]
fn function_2_args() {
    let _ = env_logger::init();

    let text = r"
\x y -> 1 #Int+ x #Int+ y
";
    let result = support::typecheck(text);
    let expected = Ok(Type::function(vec![typ("Int"), typ("Int")], typ("Int")));

    assert_eq!(result, expected);
}

#[test]
fn type_decl() {
    let _ = env_logger::init();

    let text = r"
type Test = { x: Int } in { x = 0 }
";
    let result = support::typecheck(text);
    let expected = Ok(alias(
        "Test",
        &[],
        Type::record(vec![], vec![Field::new(intern("x"), typ("Int"))]),
    ));

    assert_eq!(result, expected);
}

#[test]
fn type_decl_multiple() {
    let _ = env_logger::init();

    let text = r"
type Test = Int -> Int
and Test2 = | Test2 Test
in Test2 (\x -> x #Int+ 2)
";
    let result = support::typecheck(text);
    let test = AliasData::new(
        intern("Test"),
        vec![],
        Type::function(vec![typ("Int")], typ("Int")),
    );
    let test2 = AliasData::new(
        intern("Test2"),
        vec![],
        Type::variant(vec![
            Field {
                name: intern("Test2"),
                typ: Type::function(vec![typ("Test")], typ("Test2")),
            },
        ]),
    );
    let expected = Ok(Alias::group(vec![test, test2])[1].as_type().clone());

    assert_req!(result, expected);
}

#[test]
fn record_type_simple() {
    let _ = env_logger::init();

    let text = r"
type T = { y: Int } in
let f: T -> Int = \x -> x.y in { y = f { y = 123 } }
";
    let result = support::typecheck(text);
    let expected = Ok(alias(
        "T",
        &[],
        Type::record(vec![], vec![Field::new(intern("y"), typ("Int"))]),
    ));

    assert_eq!(result, expected);
}

#[test]
fn let_binding_type() {
    let _ = env_logger::init();

    let env = MockEnv::new();
    let text = r"
let f: a -> b -> a = \x y -> x in f 1.0 ()
";
    let (expr, result) = support::typecheck_expr(text);
    let expected = Ok(typ("Float"));

    assert_req!(result, expected);
    match expr.value {
        ast::Expr::LetBindings(ref bindings, _) => {
            assert_eq!(
                bindings[0].expr.env_type_of(&env).to_string(),
                "a -> b -> a"
            );
        }
        _ => assert!(false),
    }
}
#[test]
fn let_binding_recursive() {
    let _ = env_logger::init();

    let text = r"
let fac x = if x #Int== 0 then 1 else x #Int* fac (x #Int- 1) in fac
";
    let (_, result) = support::typecheck_expr(text);
    let expected = Ok(Type::function(vec![typ("Int")], typ("Int")));

    assert_eq!(result, expected);
}
#[test]
fn let_binding_mutually_recursive() {
    let _ = env_logger::init();

    let text = r"
let f x = if x #Int< 0
      then x
      else g x
and g x = f (x #Int- 1)
in g 5
";
    let (_, result) = support::typecheck_expr(text);
    let expected = Ok(typ("Int"));

    assert_eq!(result, expected);
}

macro_rules! assert_match {
    ($i: expr, $p: pat => $e: expr) => {
        match $i {
            $p => $e,
            ref x => assert!(false, "Expected {}, found {:?}", stringify!($p), x)
        }
    };
}

#[test]
fn let_binding_general_mutually_recursive() {
    let _ = env_logger::init();

    let text = r"
let test x = (1 #Int+ 2) #Int+ test2 x
and test2 x = 2 #Int+ test x
in test2 1";
    let (expr, result) = support::typecheck_expr(text);
    let expected = Ok(typ("Int"));

    assert_req!(result, expected);
    assert_match!(expr.value, ast::Expr::LetBindings(ref binds, _) => {
        assert_eq!(binds.len(), 2);
        assert_match!(**binds[0].resolved_type.remove_forall(), Type::App(_, ref args) => {
            assert_match!(*args[0], Type::Generic(_) => ())
        });
        assert_match!(**binds[1].resolved_type.remove_forall(), Type::App(_, ref args) => {
            assert_match!(*args[0], Type::Generic(_) => ())
        });
    });
}

#[test]
fn primitive_error() {
    let _ = env_logger::init();

    let text = r"
1 #Int== 2.2
";
    let result = support::typecheck(text);

    assert!(result.is_err());
}

#[test]
fn binop_as_function() {
    let _ = env_logger::init();

    let text = r"
let (+) = \x y -> x #Int+ y
in 1 + 2
";
    let result = support::typecheck(text);
    let expected = Ok(typ("Int"));

    assert_eq!(result, expected);
}

#[test]
fn adt() {
    let _ = env_logger::init();

    let text = r"
type Option a = | None | Some a
in Some 1
";
    let result = support::typecheck(text);
    let expected = Ok(support::typ_a("Option", vec![typ("Int")]));

    assert_req!(result.map(make_ident_type), expected);
}

#[test]
fn case_constructor() {
    let _ = env_logger::init();

    let text = r"
type Option a = | None | Some a
in match Some 1 with
    | Some x -> x
    | None -> 2
";
    let result = support::typecheck(text);
    let expected = Ok(typ("Int"));

    assert_req!(result, expected);
}

#[test]
fn real_type() {
    let _ = env_logger::init();

    let text = r"
type Eq a = {
    (==) : a -> a -> Bool
} in

let eq_Int: Eq Int = {
    (==) = \l r -> l #Int== r
}
in eq_Int
";
    let result = support::typecheck(text);
    let bool = Type::alias(
        support::intern_unscoped("Bool"),
        Type::ident(support::intern_unscoped("Bool")),
    );
    let eq = alias(
        "Eq",
        &["a"],
        Type::record(
            vec![],
            vec![
                Field::new(
                    support::intern_unscoped("=="),
                    Type::function(vec![typ("a"), typ("a")], bool),
                ),
            ],
        ),
    );
    let expected = Ok(Type::app(eq, collect![typ("Int")]));

    assert_eq!(result, expected);
}

#[test]
fn functor_option() {
    let _ = env_logger::init();

    let text = r"
type Functor f = {
    map : forall a b . (a -> b) -> f a -> f b
} in
type Option a = | None | Some a in
let option_Functor: Functor Option = {
    map = \f x -> match x with
                    | Some y -> Some (f y)
                    | None -> None
}
in option_Functor.map (\x -> x #Int- 1) (Some 2)
";
    let result = support::typecheck(text);
    let variants = Type::variant(vec![
        Field::new(
            support::intern_unscoped("None"),
            support::typ_a("Option", vec![typ("a")]),
        ),
        Field::new(
            support::intern_unscoped("Some"),
            Type::function(vec![typ("a")], support::typ_a("Option", vec![typ("a")])),
        ),
    ]);
    let option = alias("Option", &["a"], variants);

    let expected = Ok(Type::app(option, collect![typ("Int")]));

    assert_req!(result, expected);
}

#[test]
fn app_app_unify() {
    let _ = env_logger::init();

    let text = r"
type Monad m = {
    (>>=): forall a b. m a -> (a -> m b) -> m b,
    return: forall a. a -> m a
}

type Test a = | T a

let monad_Test: Monad Test = {
    (>>=) = \ta f ->
        match ta with
            | T a -> f a,
    return = \x -> T x
}

let (>>=) = monad_Test.(>>=)

let test: Test () = T 1 >>= \x -> monad_Test.return ()

test
";
    let result = support::typecheck(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());

    let variants = Type::variant(vec![
        Field::new(
            support::intern_unscoped("T"),
            Type::function(vec![typ("a")], support::typ_a("Test", vec![typ("a")])),
        ),
    ]);
    let expected = Ok(Type::app(
        alias("Test", &["a"], variants),
        collect![Type::unit()],
    ));

    assert_eq!(result, expected);
}

#[test]
fn function_operator_type() {
    let _ = env_logger::init();

    let text = r"
let f x: ((->) Int Int) = x #Int+ 1
f
";
    let result = support::typecheck(text);
    let expected = Ok(Type::function(vec![typ("Int")], typ("Int")));

    assert_eq!(result, expected);
}

#[test]
fn function_operator_partially_applied() {
    let _ = env_logger::init();

    let text = r"
type Test f = {
    test: f Int
}
let function_test: Test ((->) a) = {
    test = \x -> 1
}
function_test.test
";
    let result = support::typecheck(text);
    let expected = Ok("forall a . a -> Int".to_string());

    assert_req!(result.map(|typ| typ.to_string()), expected);
}

#[test]
fn type_alias_function() {
    let _ = env_logger::init();

    let text = r"
type Fn a b = a -> b
in
let f: Fn String Int = \x -> 123
in f
";
    let result = support::typecheck(text);
    let function = alias("Fn", &["a", "b"], Type::function(vec![typ("a")], typ("b")));
    let args = collect![typ("String"), typ("Int")];
    let expected = Ok(Type::app(function, args));

    assert_eq!(result, expected);
}

#[test]
fn infer_mutually_recursive() {
    let _ = env_logger::init();

    let text = r"
let id x = x
and const x = \_ -> x

let c: a -> b -> a = const
c
";
    let result = support::typecheck(text);

    assert!(result.is_ok(), "{}", result.unwrap_err());
}

#[test]
fn error_mutually_recursive() {
    let _ = env_logger::init();

    let text = r"
let id x = x
and const x = \_ -> x
in const #Int+ 1
";
    let result = support::typecheck(text);
    assert!(result.is_err());
}

#[test]
fn partial_function_unify() {
    let _ = env_logger::init();

    let text = r"
type Monad m = {
    (>>=) : m a -> (a -> m b) -> m b,
    return : a -> m a
} in
type State s a = s -> { value: a, state: s }
in
let (>>=) m f: State s a -> (a -> State s b) -> State s b =
    \state ->
        let { value, state } = m state
        and m2 = f value
        in m2 state
in
let return value: a -> State s a = \state -> { value, state }
in
let monad_State: Monad (State s) = { (>>=), return }
in { monad_State }
";
    let result = support::typecheck(text);

    assert_pass!(result);
}

/// Test that not all fields are required when unifying record patterns
#[test]
fn partial_pattern() {
    let _ = env_logger::init();

    let text = r#"
let { y } = { x = 1, y = "" }
in y
"#;
    let result = support::typecheck(text);
    let expected = Ok(typ("String"));

    assert_eq!(result, expected);
}

#[test]
fn type_pattern() {
    let _ = env_logger::init();

    let text = r#"
type Test = | Test String Int in { Test, x = 1 }
"#;
    let result = support::typecheck(text);
    let variant = Type::function(vec![typ("String"), typ("Int")], typ("Test"));
    let test = Type::variant(vec![
        Field {
            name: intern("Test"),
            typ: variant,
        },
    ]);
    let types = vec![
        Field {
            name: support::intern_unscoped("Test"),
            typ: Alias::new(intern("Test"), test),
        },
    ];
    let fields = vec![
        Field {
            name: intern("x"),
            typ: typ("Int"),
        },
    ];
    let expected = Ok(Type::record(types, fields));

    assert_eq!(result.map(support::close_record), expected);
}

#[test]
fn unify_variant() {
    let _ = env_logger::init();

    let text = r#"
type Test a = | Test a
Test 1
"#;
    let result = support::typecheck(text);
    let expected = Ok(support::typ_a("Test", vec![typ("Int")]));

    assert_eq!(result.map(make_ident_type), expected);
}

#[test]
fn unify_transformer() {
    let _ = env_logger::init();

    let text = r#"
type Test a = | Test a
type Id a = | Id a
type IdT m a = m (Id a)
let return x: a -> IdT Test a = Test (Id x)
return 1
"#;
    let result = support::typecheck(text);
    let variant = |name| {
        Type::variant(vec![
            Field::new(
                intern(name),
                Type::function(vec![typ("a")], Type::app(typ(name), collect![typ("a")])),
            ),
        ])
    };
    let test = alias("Test", &["a"], variant("Test"));
    let m = Generic::new(intern("m"), Kind::function(Kind::typ(), Kind::typ()));

    let id = alias("Id", &["a"], variant("Id"));
    let id_t = Type::alias(
        intern("IdT"),
        Type::forall(
            vec![m.clone(), Generic::new(intern("a"), Kind::typ())],
            Type::app(
                Type::generic(m),
                collect![Type::app(id, collect![typ("a")])],
            ),
        ),
    );
    let expected = Ok(Type::app(id_t, collect![test, typ("Int")]));

    assert_req!(result, expected);
}

#[test]
fn normalize_function_type() {
    let _ = env_logger::init();

    let text = r#"
type Cat cat = {
    id : forall a . cat a a,
}
let cat: Cat (->) = {
    id = \x -> x,
}
let { id } = cat
let { id } = cat
let test f: (a -> m b) -> m b = test f
test id
"#;
    let result = support::typecheck(text);

    assert!(result.is_ok(), "{}", result.unwrap_err());
}

#[test]
fn mutually_recursive_types() {
    let _ = env_logger::init();

    let text = r#"
type Tree a = | Empty | Node (Data a) (Data a)
and Data a = { value: a, tree: Tree a }
in
let rhs : Data Int = {
    value = 123,
    tree = Node { value = 0, tree = Empty } { value = 42, tree = Empty }
}
in Node { value = 1, tree = Empty } rhs
"#;
    let result = support::typecheck(text);
    let expected = Ok(support::typ_a("Tree", vec![typ("Int")]));

    assert_eq!(result.map(make_ident_type), expected);
}

#[test]
fn field_access_through_multiple_aliases() {
    let _ = env_logger::init();

    let text = r#"
type Test1 = { x: Int }
and Test2 = Test1

let t: Test2 = { x = 1 }

t.x
"#;
    let result = support::typecheck(text);
    let expected = Ok(typ("Int"));

    assert_eq!(result, expected);
}

#[test]
fn unify_equal_hkt_aliases() {
    let _ = env_logger::init();

    let text = r#"
type M a = | M a
and M2 a = M a
and HKT m = { x: m Int }
in
let eq: a -> a -> Int = \x y -> 1
and t: HKT M = { x = M 1 }
and u: HKT M2 = t
in eq t u
"#;
    let result = support::typecheck(text);

    assert!(result.is_ok(), "{}", result.unwrap_err());
}

#[test]
fn overloaded_bindings() {
    let _ = env_logger::init();

    let text = r#"
let (+) x y = x #Int+ y
in
let (+) x y = x #Float+ y
in
{ x = 1 + 2, y = 1.0 + 2.0 }
"#;
    let result = support::typecheck(text);
    let fields = vec![
        Field::new(intern("x"), typ("Int")),
        Field::new(intern("y"), typ("Float")),
    ];
    let expected = Ok(Type::record(vec![], fields));

    assert_req!(result.map(support::close_record), expected);
}

#[test]
fn overloaded_record_binding() {
    let _ = env_logger::init();

    let text = r#"
let { f } = { f = \x -> x #Int+ 1 }
in
let { f } = { f = \x -> x #Float+ 1.0 }
in
{ x = f 1, y = f 1.0 }
"#;
    let result = support::typecheck(text);
    let fields = vec![
        Field::new(intern("x"), typ("Int")),
        Field::new(intern("y"), typ("Float")),
    ];
    let expected = Ok(Type::record(vec![], fields));

    assert_req!(result.map(support::close_record), expected);
}

#[test]
fn overloaded_bindings_3() {
    let _ = env_logger::init();

    let text = r#"
let (+) x y = x #Int+ y
let (+) x y = x #Float+ y
let (+) x y : () -> () -> () = y

{ x = 1 + 2, y = 1.0 + 2.0, z = () + () }
"#;
    let result = support::typecheck(text);
    let fields = vec![
        Field::new(intern("x"), typ("Int")),
        Field::new(intern("y"), typ("Float")),
        Field::new(intern("z"), Type::unit()),
    ];
    let expected = Ok(Type::record(vec![], fields));

    assert_req!(result.map(support::close_record), expected);
}

#[test]
fn as_pattern() {
    let _ = env_logger::init();

    let text = r#"
match 1 with
| x@y -> x #Int+ y
"#;
    let result = support::typecheck(text);
    let expected = Ok(Type::int());

    assert_req!(result, expected);
}

#[test]
fn as_pattern_record() {
    let _ = env_logger::init();

    let text = r#"
match { y = 1 } with
| x@{ y } -> x
"#;
    let result = support::typecheck(text);
    let fields = vec![Field::new(intern("y"), typ("Int"))];
    let expected = Ok(Type::record(vec![], fields));

    assert_req!(result, expected);
}

#[test]
fn do_expression_simple() {
    let _ = env_logger::init();

    let text = r#"
type Test a = { x : a }
let flat_map f x = { x = f x.x }
let test x: a -> Test a = { x }

do x = test 1
test ""
"#;
    let result = support::typecheck(text);
    let expected = Ok(Type::app(
        alias(
            "Test",
            &["a"],
            Type::record(vec![], vec![Field::new(intern("x"), typ("a"))]),
        ),
        collect![typ("String")],
    ));

    assert_req!(result.map(support::close_record), expected);
}

#[test]
fn do_expression_use_binding() {
    let _ = env_logger::init();

    let text = r#"
type Test a = { x : a }
let flat_map f x : (a -> Test b) -> Test a -> Test b = f x.x
let test x : a -> Test a = { x }

do x = test 1
test (x #Int+ 2)
"#;
    let result = support::typecheck(text);
    let expected = Ok(Type::app(
        alias(
            "Test",
            &["a"],
            Type::record(vec![], vec![Field::new(intern("x"), typ("a"))]),
        ),
        collect![typ("Int")],
    ));

    assert_req!(result.map(support::close_record), expected);
}

#[test]
fn eq_unresolved_constraint_bug() {
    let _ = env_logger::init();

    let text = r#"
type Eq a = { (==) : a -> a -> Bool }
type List a = | Nil | Cons a (List a)

let (==): Int -> Int -> Bool = \x y -> True

let eq a : Eq a -> Eq (List a) =
    let (==) l r : List a -> List a -> Bool =
        match (l, r) with
        | (Cons x xs, Cons y ys) -> a.(==) x y
        | _ -> False
    { (==) }
()
"#;
    let result = support::typecheck(text);

    assert!(result.is_ok(), "{}", result.unwrap_err());
}

#[test]
fn pattern_match_nested_parameterized_type() {
    let _ = env_logger::init();

    let text = r#"
type Test a = | Test a
match Test { x = 1 } with
| Test { x } -> x
"#;
    let result = support::typecheck(text);

    assert!(result.is_ok(), "{}", result.unwrap_err());
}
