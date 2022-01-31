extern crate env_logger;

extern crate base;
extern crate parser;
extern crate check;

use base::ast;
use base::ast::Typed;
use base::symbol::Symbol;
use base::types::{Type, TcType};
use base::types;

mod functions;
use functions::*;

macro_rules! assert_pass {
    ($e: expr) => {{
        if !$e.is_ok() {
            panic!("assert_pass: {}", $e.unwrap_err());
        }
    }}
}
macro_rules! assert_err {
    ($e: expr, $($id: pat),+) => {{
        use check::typecheck::TypeError::*;
        #[allow(unused_imports)]
        use check::unify::Error::{TypeMismatch, Occurs, Other};
        #[allow(unused_imports)]
        use check::unify_type::TypeError::FieldMismatch;
        let symbols = get_local_interner();
        match $e {
            Ok(x) => assert!(false, "Expected error, got {}",
                             types::display_type(&*symbols.borrow(), &x)),
            Err(err) => {
                let mut iter = err.errors.iter();
                $(
                match iter.next() {
                    Some(&ast::Spanned { value: $id, .. }) => (),
                    _ => assert!(false, "Found errors:\n{}\nbut expected {}",
                                        err, stringify!($id))
                }
                )+
                assert!(iter.count() == 0, "Found more errors than expected\n{}", err);
            }
        }
    }}
}

macro_rules! assert_unify_err {
    ($e: expr, $($id: pat),+) => {{
        use check::typecheck::TypeError::*;
        #[allow(unused_imports)]
        use check::unify::Error::{TypeMismatch, Occurs, Other};
        #[allow(unused_imports)]
        use check::unify_type::TypeError::FieldMismatch;
        let symbols = get_local_interner();
        match $e {
            Ok(x) => assert!(false, "Expected error, got {}",
                             types::display_type(&*symbols.borrow(), &x)),
            Err(err) => {
                for err in err.errors.iter() {
                    match *err {
                        ast::Spanned { value: Unification(_, _, ref errors), .. } => {
                            let mut iter = errors.iter();
                            $(
                            match iter.next() {
                                Some(&$id) => (),
                                _ => assert!(false, "Found errors:\n{}\nbut expected {}",
                                                    err, stringify!($id))
                            }
                            )+
                            assert!(iter.count() == 0,
                                    "Found more errors than expected\n{}",
                                    err);
                        }
                        _ => assert!(false,
                                     "Found errors:\n{}\nbut expected an unification error",
                                     err)
                    }
                }
            }
        }
    }}
}

fn typ(s: &str) -> TcType {
    assert!(s.len() != 0);
    typ_a(s, Vec::new())
}

fn typ_a<T>(s: &str, args: Vec<T>) -> T where T: From<Type<Symbol, T>> {
    assert!(s.len() != 0);
    let is_var = s.chars().next().unwrap().is_lowercase();
    match s.parse() {
        Ok(b) => Type::builtin(b),
        Err(()) if is_var => {
            Type::generic(types::Generic {
                kind: types::Kind::star(),
                id: intern(s),
            })
        }
        Err(()) => if args.len() == 0 { Type::id(intern(s)) } else { Type::data(Type::id(intern(s)), args) }
    }
}

#[test]
fn function_type_new() {
    let text = r"
\x -> x
";
    let result = typecheck(text);
    assert!(result.is_ok());
    match *result.unwrap() {
        Type::Function(_, _) => {
            assert!(true);
        }
        _ => assert!(false),
    }
}

#[test]
fn char_literal() {
    let _ = ::env_logger::init();
    let text = r"
'a'
";
    let result = typecheck(text);
    assert_eq!(result, Ok(Type::char()));
}

#[test]
fn function_2_args() {
    let _ = ::env_logger::init();
    let text = r"
\x y -> 1 #Int+ x #Int+ y
";
    let result = typecheck(text);
    assert_eq!(result,
               Ok(Type::function(vec![typ("Int"), typ("Int")], typ("Int"))));
}

#[test]
fn type_decl() {
    let _ = ::env_logger::init();
    let text = r"
type Test = { x: Int } in { x = 0 }
";
    let result = typecheck(text);
    assert_eq!(result, Ok(typ("Test")));
}

#[test]
fn type_decl_multiple() {
    let _ = ::env_logger::init();
    let text = r"
type Test = Int -> Int
and Test2 = | Test2 Test
in Test2 (\x -> x #Int+ 2)
";
    let result = typecheck(text);
    assert_eq!(result, Ok(typ("Test2")));
}

#[test]
fn record_type_simple() {
    let _ = ::env_logger::init();
    let text = r"
type T = { y: Int } in
let f: T -> Int = \x -> x.y in { y = f { y = 123 } }
";
    let result = typecheck(text);
    assert_eq!(result, Ok(typ("T")));
}

#[test]
fn let_binding_type() {
    let _ = ::env_logger::init();
    let text = r"
let f: a -> b -> a = \x y -> x in f 1.0 ()
";
    let (expr, result) = typecheck_expr(text);
    assert_eq!(result, Ok(typ("Float")));
    match expr.value {
        ast::Expr::Let(ref bindings, _) => {
            assert_eq!(bindings[0].expression.type_of(),
                       Type::function(vec![typ("a"), typ("b")], typ("a")));
        }
        _ => assert!(false),
    }
}
#[test]
fn let_binding_recursive() {
    let _ = ::env_logger::init();
    let text = r"
let fac x = if x #Int== 0 then 1 else x #Int* fac (x #Int- 1) in fac
";
    let (_, result) = typecheck_expr(text);
    assert_eq!(result, Ok(Type::function(vec![typ("Int")], typ("Int"))));
}
#[test]
fn let_binding_mutually_recursive() {
    let _ = ::env_logger::init();
    let text = r"
let f x = if x #Int< 0
      then x
      else g x
and g x = f (x #Int- 1)
in g 5
";
    let (_, result) = typecheck_expr(text);
    assert_eq!(result, Ok(typ("Int")));
}

macro_rules! assert_m {
($i: expr, $p: pat => $e: expr) => {
    match $i {
        $p => $e,
        ref x => assert!(false, "Unexpected {}, found {:?}", stringify!($p), x)
    }
}
}

#[test]
fn let_binding_general_mutually_recursive() {
    let _ = ::env_logger::init();
    let text = r"
let test x = (1 #Int+ 2) #Int+ test2 x
and test2 x = 2 #Int+ test x
in test2 1";
    let (expr, result) = typecheck_expr(text);
    assert_eq!(result, Ok(typ("Int")));
    assert_m!(expr.value, ast::Expr::Let(ref binds, _) => {
        assert_eq!(binds.len(), 2);
        assert_m!(*binds[0].type_of(), Type::Function(ref args, _) => {
            assert_m!(*args[0], Type::Generic(_) => ())
        });
        assert_m!(*binds[1].type_of(), Type::Function(ref args, _) => {
            assert_m!(*args[0], Type::Generic(_) => ())
        });
    });
}

#[test]
fn primitive_error() {
    let _ = ::env_logger::init();
    let text = r"
1 #Int== 2.2
";
    let result = typecheck(text);
    assert!(result.is_err());
}
#[test]
fn binop_as_function() {
    let _ = ::env_logger::init();
    let text = r"
let (+) = \x y -> x #Int+ y
in 1 + 2
";
    let result = typecheck(text);
    assert_eq!(result, Ok(typ("Int")));
}
#[test]
fn adt() {
    let _ = ::env_logger::init();
    let text = r"
type Option a = | None | Some a
in Some 1
";
    let result = typecheck(text);
    assert_eq!(result, Ok(typ_a("Option", vec![typ("Int")])));
}
#[test]
fn case_constructor() {
    let _ = ::env_logger::init();
    let text = r"
type Option a = | None | Some a
in match Some 1 with
    | Some x -> x
    | None -> 2
";
    let result = typecheck(text);
    assert_eq!(result, Ok(typ("Int")));
}
#[test]
fn real_type() {
    let _ = ::env_logger::init();
    let text = r"
type Eq a = {
    (==) : a -> a -> Bool
} in

let eq_Int: Eq Int = {
    (==) = \l r -> l #Int== r
}
in eq_Int
";
    let result = typecheck(text);
    assert_eq!(result, Ok(typ_a("Eq", vec![typ("Int")])));
}

#[test]
fn functor() {
    let _ = ::env_logger::init();
    let text = r"
type Functor f = {
    map : (a -> b) -> f a -> f b
} in
type Option a = | None | Some a in
let option_Functor: Functor Option = {
    map = \f x -> match x with
                    | Some y -> Some (f y)
                    | None -> None
}
in option_Functor.map (\x -> x #Int- 1) (Some 2)
";
    let result = typecheck(text);
    assert_eq!(result, Ok(typ_a("Option", vec![typ("Int")])));
}

#[test]
fn app_app_unify() {
    let _ = ::env_logger::init();
    let text = r"
type Monad m = {
    (>>=): m a -> (a -> m b) -> m b,
    return: a -> m a
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
    let result = typecheck(text);
    assert!(result.is_ok(), "{}", result.unwrap_err());
    assert_eq!(result, Ok(typ_a("Test", vec![Type::unit()])));
}

#[test]
fn record_missing_field() {
    let _ = ::env_logger::init();
    let text = r"
match { x = 1 } with
| { x, y } -> 1
";
    let result = typecheck(text);
    assert_err!(result, UndefinedField(..));
}

#[test]
fn type_alias_function() {
    let _ = ::env_logger::init();
    let text = r"
type Fn a b = a -> b
in
let f: Fn String Int = \x -> 123
in f
";
    let result = typecheck(text);
    assert_eq!(result, Ok(typ_a("Fn", vec![typ("String"), typ("Int")])));
}

#[test]
fn infer_mutually_recursive() {
    let _ = ::env_logger::init();
    let text = r"
let id x = x
and const x = \_ -> x

let c: a -> b -> a = const
c
";
    let result = typecheck(text);
    assert!(result.is_ok());
}

#[test]
fn error_mutually_recursive() {
    let _ = ::env_logger::init();
    let text = r"
let id x = x
and const x = \_ -> x
in const #Int+ 1
";
    let result = typecheck(text);
    assert!(result.is_err());
}

#[test]
fn infer_ord_int() {
    let _ = ::env_logger::init();
    let text = r#"
type Ordering = | LT | EQ | GT
type Ord a = {
    compare : a -> a -> Ordering
}
let ord_Int = {
    compare = \l r ->
        if l #Int< r
        then LT
        else if l #Int== r
        then EQ
        else GT
}
let make_Ord ord =
    let compare = ord.compare
    in {
        (<=) = \l r ->
            match compare l r with
                | LT -> True
                | EQ -> True
                | GT -> False
    }
let (<=) = (make_Ord ord_Int).(<=)

"" <= ""
"#;
    let result = typecheck(text);
    assert_unify_err!(result, TypeMismatch(..), TypeMismatch(..));
}

#[test]
fn partial_function_unify() {
    let _ = ::env_logger::init();
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
    let result = typecheck(text);
    assert_pass!(result);
}

///Test that not all fields are required when unifying record patterns
#[test]
fn partial_pattern() {
    let _ = ::env_logger::init();
    let text = r#"
let { y } = { x = 1, y = "" }
in y
"#;
    let result = typecheck(text);
    assert_eq!(result, Ok(typ("String")));
}

#[test]
fn type_pattern() {
    let _ = ::env_logger::init();
    let text = r#"
type Test = | Test String Int in { Test, x = 1 }
"#;
    let result = typecheck(text);
    let variant = Type::function(vec![typ("String"), typ("Int")], typ("Test"));
    let test = Type::variants(vec![(intern("Test"), variant)]);
    assert_eq!(result,
               Ok(Type::record(vec![types::Field {
                                        name: intern_unscoped("Test"),
                                        typ: types::Alias {
                                            name: intern("Test"),
                                            args: vec![],
                                            typ: Some(test),
                                        },
                                    }],
                               vec![types::Field {
                                        name: intern("x"),
                                        typ: typ("Int"),
                                    }])));
}

#[test]
fn undefined_type() {
    let _ = ::env_logger::init();
    let text = r#"
let x =
    type Test = | Test String Int
    in { Test, x = 1 }
in
type Test2 = Test
in x
"#;
    let result = typecheck(text);
    assert_err!(result, UndefinedType(..));
}

#[test]
fn record_type_out_of_scope() {
    let _ = ::env_logger::init();
    let text = r#"
let test =
    type Test = { x: Int }
    in let y: Test = { x = 0 }
    in y
in match test with
    | { x } -> x
"#;
    let result = typecheck(text);
    assert_err!(result, Unification(..));
}

#[test]
fn undefined_variant() {
    let _ = ::env_logger::init();
    let text = r#"
let x =
    type Test = | Test String Int
    { Test, x = 1 }
Test "" 2
"#;
    let result = typecheck(text);
    assert_err!(result, UndefinedVariable(..));
}

#[test]
fn unify_variant() {
    let _ = ::env_logger::init();
    let text = r#"
type Test a = | Test a
Test 1
"#;
    let result = typecheck(text);
    assert_eq!(result, Ok(typ_a("Test", vec![typ("Int")])));
}

#[test]
fn unify_transformer() {
    let _ = ::env_logger::init();
    let text = r#"
type Test a = | Test a
type Id a = | Id a
type IdT m a = m (Id a)
let return x: a -> IdT Test a = Test (Id x)
return 1
"#;
    let result = typecheck(text);
    assert_eq!(result, Ok(typ_a("IdT", vec![typ("Test"), typ("Int")])));
}

// TODO Uncomment when backtracking is working when unifying #[test]
fn unify_transformer2() {
    let _ = ::env_logger::init();
    let text = r#"
type Option a = | None | Some a in
type Monad m = {
    return : a -> m a
} in
let monad_Option: Monad Option = {
    return = \x -> Some x
} in
type OptionT m a = m (Option a)
in
let monad_OptionT m: Monad m1 -> Monad (OptionT m1) =
    let return x: b -> OptionT m1 b = m.return (Some x)
    in {
        return
    }
in 1
"#;
    let result = typecheck(text);
    println!("{}", result.as_ref().unwrap_err());
    assert_eq!(result, Ok(typ("Int")));
}

#[test]
fn mutually_recursive_types() {
    let _ = ::env_logger::init();
    let text = r#"
type Tree a = | Empty | Node (Data a) (Data a)
and Data a = { value: a, tree: Tree a }
in
let rhs = { value = 123, tree = Node { value = 0, tree = Empty } { value = 42, tree = Empty } }
in Node { value = 1, tree = Empty } rhs
"#;
    let result = typecheck(text);
    assert_eq!(result, Ok(typ_a("Tree", vec![typ("Int")])));
}

#[test]
fn mutually_recursive_types_error() {
    let _ = ::env_logger::init();
    let text = r#"
type List a = | Empty | Node (a (Data a))
and Data a = { value: a, list: List a }
in 1
"#;
    let result = typecheck(text);
    assert_err!(result, KindError(TypeMismatch(..)));
}

#[test]
fn field_access_through_multiple_aliases() {
    let _ = ::env_logger::init();
    let text = r#"
type Test1 = { x: Int }
and Test2 = Test1

let t: Test2 = { x = 1 }

t.x
"#;
    let result = typecheck(text);
    assert_eq!(result, Ok(typ("Int")));
}

#[test]
fn unify_equal_hkt_aliases() {
    let _ = ::env_logger::init();
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
    let result = typecheck(text);
    assert_eq!(result, Ok(typ("Int")));
}

#[test]
fn duplicate_type_definition() {
    let _ = ::env_logger::init();
    let text = r#"
type Test = Int
in
type Test = Float
in 1
"#;
    let result = typecheck(text);
    assert_err!(result, DuplicateTypeDefinition(..));
}

#[test]
fn overloaded_bindings() {
    let _ = ::env_logger::init();
    let text = r#"
let (+) x y = x #Int+ y
in
let (+) x y = x #Float+ y
in
{ x = 1 + 2, y = 1.0 + 2.0 }
"#;
    let result = typecheck(text);
    assert_eq!(result,
               Ok(Type::record(vec![],
                               vec![types::Field {
                                        name: intern("x"),
                                        typ: typ("Int"),
                                    },
                                    types::Field {
                                        name: intern("y"),
                                        typ: typ("Float"),
                                    }])));
}

#[test]
fn overloaded_record_binding() {
    let _ = ::env_logger::init();
    let text = r#"
let { f } = { f = \x -> x #Int+ 1 }
in
let { f } = { f = \x -> x #Float+ 1.0 }
in
{ x = f 1, y = f 1.0 }
"#;
    let result = typecheck(text);
    assert_eq!(result,
               Ok(Type::record(vec![],
                               vec![types::Field {
                                        name: intern("x"),
                                        typ: typ("Int"),
                                    },
                                    types::Field {
                                        name: intern("y"),
                                        typ: typ("Float"),
                                    }])));
}

#[test]
fn no_matching_overloaded_binding() {
    let _ = ::env_logger::init();
    let text = r#"
let f x = x #Int+ 1
in
let f x = x #Float+ 1.0
in f ""
"#;
    let result = typecheck(text);
    assert_err!(result, Rename(..));
}

#[test]
fn not_enough_information_to_decide_overload() {
    let _ = ::env_logger::init();
    let text = r#"
let f x = x #Int+ 1
let f x = x #Float+ 1.0
\x -> f x
"#;
    let result = typecheck(text);
    assert_err!(result, Rename(..));
}

#[test]
fn type_field_mismatch() {
    let _ = ::env_logger::init();
    let text = r#"
if True then
    type Test = Int
    { Test }
else
    type Test = Float
    { Test }
"#;
    let result = typecheck(text);
    assert_unify_err!(result, TypeMismatch(..));
}

#[test]
fn module() {
    let _ = ::env_logger::init();
    let text = r"
type SortedList a = | Cons a (SortedList a)
                | Nil
in \(<) ->
    let empty = Nil
    let insert x xs =
        match xs with
        | Nil -> Cons x Nil
        | Cons y ys -> if x < y
                       then Cons x xs
                       else Cons y (insert x ys)
    let ret : { empty: SortedList a, insert: a -> SortedList a -> SortedList a }
        = { empty, insert }
    ret
";
    let result = typecheck(text);
    assert!(result.is_ok());
}

#[test]
fn call_error_span() {
    let _ = ::env_logger::init();
    let text = r#"
let f x = x #Int+ 1
in f "123"
"#;
    let result = typecheck(text);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.errors.len(), 1);
    assert_eq!(err.errors[0].span, ast::Span {
        start: ast::Location { row: 3, column: 6, absolute: 0 },
        end: ast::Location { row: 3, column: 11, absolute: 0 },
    });
}
