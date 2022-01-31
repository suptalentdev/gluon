#[macro_use]
extern crate collect_mac;
extern crate env_logger;
#[macro_use]
extern crate pretty_assertions;

extern crate gluon_base as base;
extern crate gluon_check as check;
extern crate gluon_parser as parser;

use base::symbol::Symbol;
use base::types::{ArcType, Type};

use check::typecheck::TypeError;
use check::rename::RenameError;

#[macro_use]
mod support;

#[test]
fn record_missing_field() {
    let _ = env_logger::init();
    let text = r"
match { x = 1 } with
| { x, y } -> 1
";
    let result = support::typecheck(text);

    assert_unify_err!(result, Other(MissingFields(..)));
}

#[test]
fn undefined_type_not_in_scope() {
    let _ = env_logger::init();
    let text = r#"
let x =
    type Test = | Test String Int
    in { Test, x = 1 }
in
type Test2 = Test
in x
"#;
    let result = support::typecheck(text);

    assert_err!(result, UndefinedType(..));
}

#[test]
fn undefined_variant() {
    let _ = env_logger::init();
    let text = r#"
let x =
    type Test = | Test String Int
    { Test, x = 1 }
Test "" 2
"#;
    let result = support::typecheck(text);

    assert_err!(result, UndefinedVariable(..));
}

#[test]
fn mutually_recursive_types_error() {
    let _ = env_logger::init();
    let text = r#"
type List a = | Empty | Node (a (Data a))
and Data a = { value: a, list: List a }
in 1
"#;
    let result = support::typecheck(text);

    assert_err!(result, KindError(TypeMismatch(..)));
}

#[test]
fn unpack_field_which_does_not_exist() {
    let _ = env_logger::init();
    let text = r#"
let { y } = { x = 1 }
2
"#;
    let result = support::typecheck(text);

    assert_unify_err!(result, Other(MissingFields(..)));
}

#[test]
fn unpack_type_field_which_does_not_exist() {
    let _ = env_logger::init();
    let text = r#"
type Test = Int
let { Test2 } = { Test }
2
"#;
    let result = support::typecheck(text);

    assert_err!(result, UndefinedField(..));
}

#[test]
fn duplicate_type_definition() {
    let _ = env_logger::init();
    let text = r#"
type Test = Int
in
type Test = Float
in 1
"#;
    let result = support::typecheck(text);

    assert_err!(result, DuplicateTypeDefinition(..));
}

#[test]
fn no_matching_overloaded_binding() {
    let _ = env_logger::init();
    let text = r#"
let f x = x #Int+ 1
let f x = x #Float+ 1.0
let f x : () -> () = ()
f ""
"#;
    let result = support::typecheck(text);

    assert_multi_unify_err!(
        result,
        [Substitution(Constraint(..))],
        [Substitution(Constraint(..))]
    );
}

#[test]
fn no_matching_binop_binding() {
    let _ = env_logger::init();
    let text = r#"
let (++) x y = x #Int+ y
let (++) x y = x #Float+ y
"" ++ ""
"#;
    let result = support::typecheck(text);

    assert_multi_unify_err!(
        result,
        [Substitution(Constraint(..))],
        [Substitution(Constraint(..))]
    );
}

// TODO Determine what the correct semantics is for this case
#[ignore]
#[test]
fn not_enough_information_to_decide_overload() {
    let _ = env_logger::init();
    let text = r#"
let f x = x #Int+ 1
let f x = x #Float+ 1.0
\x -> f x
"#;
    let result = support::typecheck(text);

    assert_unify_err!(result, Substitution(Constraint(..)));
}

#[test]
fn unable_to_resolve_implicit_without_attribute() {
    let _ = env_logger::init();
    let text = r#"
let f x y : [a] -> a -> a = x
let i = 123
f 1
"#;
    let result = support::typecheck(text);

    assert_err!(result, Rename(RenameError::UnableToResolveImplicit(..)));
}

#[test]
fn type_field_mismatch() {
    let _ = env_logger::init();
    let text = r#"
if True then
    type Test = Int
    { Test }
else
    type Test = Float
    { Test }
"#;
    let result = support::typecheck(text);

    assert_unify_err!(result, TypeMismatch(..));
}

#[test]
fn arguments_need_to_be_instantiated_before_any_access() {
    let _ = env_logger::init();
    // test_fn: forall a. (a -> ()) -> ()
    // To allow any type to be passed to `f` it should be
    // test_fn: (forall a. a -> ()) -> ()
    let text = r#"
let test_fn f: (a -> ()) -> () =
    f 2.0
1
"#;
    let result = support::typecheck(text);

    assert_unify_err!(result, TypeMismatch(..));
}

#[test]
fn infer_ord_int() {
    let _ = env_logger::init();
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
let make_Ord ord : forall a . Ord a -> _ =
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
    let result = support::typecheck(text);

    assert_multi_unify_err!(result, [TypeMismatch(..)], [TypeMismatch(..)]);
}

#[test]
fn recursive_types_with_differing_aliases() {
    let _ = env_logger::init();
    let text = r"
type Option a = | None | Some a
type R1 = Option R1
and R2 = Option R2

let x: R1 = None
let y: R2 = x
y
";
    let result = support::typecheck(text);

    assert_unify_err!(result, Other(SelfRecursive(..)));
}

#[test]
fn detect_self_recursive_aliases() {
    let _ = env_logger::init();
    let text = r"
type A a = A a

let g x: A a -> () = x
1
";
    let result = support::typecheck(text);

    assert_unify_err!(result, Other(SelfRecursive(..)));
}

#[test]
fn declared_generic_variables_may_not_make_outer_bindings_more_general() {
    let _ = ::env_logger::init();
    let text = r#"
let make m =
    let m2: m = m
    let _ = m2 1
    m

make 2
"#;
    let result = support::typecheck(text);
    assert!(result.is_err());
}

#[test]
fn duplicate_fields() {
    let _ = ::env_logger::init();
    let text = r#"
type Test = Int
let x = ""
{ Test, Test, x = 1, x }
"#;
    let result = support::typecheck(text);
    assert_err!(result, DuplicateField(..), DuplicateField(..));
}

#[test]
fn duplicate_fields_pattern() {
    let _ = ::env_logger::init();
    let text = r#"
type Test = Int
let { Test, Test, x = y, x } = { Test, x = 1 }
()
"#;
    let result = support::typecheck(text);
    assert_err!(result, DuplicateField(..), DuplicateField(..));
}

#[test]
fn type_alias_with_explicit_type_kind() {
    let _ = ::env_logger::init();
    let text = r#"
type Test (a : Type) = a
type Foo a = a
type Bar = Test Foo
()
"#;
    let result = support::typecheck(text);
    assert_err!(result, KindError(TypeMismatch(..)));
}

#[test]
fn type_alias_with_explicit_row_kind() {
    let _ = ::env_logger::init();
    let text = r#"
type Test (a : Row) = a
type Bar = Test Int
()
"#;
    let result = support::typecheck(text);
    assert_err!(
        result,
        KindError(TypeMismatch(..)),
        KindError(TypeMismatch(..))
    );
}

#[test]
fn type_alias_with_explicit_function_kind() {
    let _ = ::env_logger::init();
    let text = r#"
type Test (a : Type -> Type) = a Int
type Foo = Test Int
()
"#;
    let result = support::typecheck(text);
    assert_err!(result, KindError(TypeMismatch(..)));
}

#[test]
fn type_error_span() {
    use base::pos::Span;

    let _ = ::env_logger::init();
    let text = r#"
let y = 1.0
y
"#;
    let result = support::typecheck_expr_expected(text, Some(&Type::int())).1;
    let errors: Vec<_> = result.unwrap_err().errors().into();
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].span.map(|loc| loc.absolute),
        Span::new(13.into(), 14.into())
    );
}

#[test]
fn issue_286() {
    let _ = ::env_logger::init();
    let text = r#"
let Test = 1
1
"#;
    let result = support::typecheck(text);
    assert_err!(result, UndefinedVariable(..));
}

#[test]
fn no_inference_variable_in_error() {
    let _ = ::env_logger::init();
    let text = r#"
() 1
"#;
    let result = support::typecheck(text);

    assert_eq!(
        &*format!("{}", result.unwrap_err()).replace("\t", "        "),
        r#"test:Line: 2, Column: 1: Expected the following types to be equal
Expected: a -> a0
Found: ()
1 errors were found during unification:
Types do not match:
    Expected: a -> a0
    Found: ()
() 1
^~~~
"#
    );
}

#[test]
fn alias_mismatch() {
    let _ = ::env_logger::init();
    let text = r#"
type A = | A Int
type B = | B Float
let eq x _ : a -> a -> a = x
eq (A 0) (B 0.0)
"#;
    let result = support::typecheck(text);

    assert_eq!(
        &*format!("{}", result.unwrap_err()).replace("\t", "        "),
        r#"test:Line: 5, Column: 11: Expected the following types to be equal
Expected: test.A
Found: test.B
1 errors were found during unification:
Types do not match:
    Expected: test.A
    Found: test.B
eq (A 0) (B 0.0)
          ^~~~~
"#
    );
}

#[test]
fn unification_error_with_empty_record_displays_good_error_message() {
    let _ = ::env_logger::init();
    let text = r#"
let f x y : a -> a -> a = x

f { } { x = 1 }
"#;

    let result = support::typecheck(text);

    assert_eq!(
        &*format!("{}", result.unwrap_err()).replace("\t", "        "),
        r#"test:Line: 4, Column: 7: Expected the following types to be equal
Expected: ()
Found: { x : Int }
1 errors were found during unification:
The type `()` lacks the following fields: x
f { } { x = 1 }
      ^~~~~~~~~
"#
    );
}

#[test]
fn long_type_error_format() {
    let long_type: ArcType = Type::function(
        vec![Type::int()],
        Type::ident(Symbol::from(
            "loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooong",
        )),
    );
    let err = TypeError::Unification(Type::int(), long_type.clone(), vec![]);
    assert_eq!(
        &*err.to_string(),
        r#"Expected the following types to be equal
Expected: Int
Found:
    Int
        -> loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooong
0 errors were found during unification:
"#
    );
}

#[test]
fn undefined_field_after_overload() {
    let _ = ::env_logger::init();
    let text = r#"
let f =
    \x g ->
        let { x } = g x
        x
let r = f { x = 0 } (\r -> { x = r.x #Int+ 1 })
r.y
"#;
    let result = support::typecheck(text);
    assert_err!(result, InvalidProjection(..));
}

#[test]
fn type_constructor_in_function_name() {
    let _ = ::env_logger::init();
    let text = r#"
let Test x = x
1
"#;
    let result = support::typecheck(text);
    assert_err!(result, Message(..));
}

#[test]
fn record_base_not_record() {
    let _ = ::env_logger::init();
    let text = r#"
{ .. 1 }
"#;
    let result = support::typecheck(text);
    assert_unify_err!(result, TypeMismatch(..));
}

#[test]
fn undefined_type_variable() {
    let _ = ::env_logger::init();
    let text = r#"
type Test = a
()
"#;
    let result = support::typecheck(text);
    assert_err!(result, UndefinedVariable(..));
}

#[test]
fn undefined_type_variable_in_enum() {
    let _ = ::env_logger::init();
    let text = r#"
type Test = | Test a
()
"#;
    let result = support::typecheck(text);
    assert_err!(result, UndefinedVariable(..));
}

#[test]
fn make_with_explicit_types_with_wrong_variable() {
    let _ = ::env_logger::init();

    let text = r#"
let make x : b -> _ =
    let f y : b -> a = x
    { f }

make
"#;
    let result = support::typecheck(text);

    assert!(result.is_err(), "{}", result.unwrap_err());
}

#[test]
fn double_type_variable_unification_bug() {
    let _ = ::env_logger::init();

    let text = r#"
\k ->
    let { k } = k
    insert k
"#;
    let result = support::typecheck(text);

    assert_err!(result, UndefinedVariable(..));
}

#[test]
fn do_expression_undefined_flat_map() {
    let _ = ::env_logger::init();

    let text = r#"
do x = 1
2
"#;
    let result = support::typecheck(text);

    assert_err!(result, UndefinedVariable(..));
}

#[test]
fn do_expression_type_mismatch() {
    let _ = ::env_logger::init();

    let text = r#"
let flat_map f = 1
do x = 1
2
"#;
    let result = support::typecheck(text);

    assert_unify_err!(result, TypeMismatch(..));
}

#[test]
fn undefined_type_in_variant() {
    let _ = ::env_logger::init();

    let text = r#"
type Test = | Test In
2
"#;
    let result = support::typecheck(text);

    assert_err!(result, UndefinedType(..));
}

#[test]
fn foldable_bug() {
    let _ = ::env_logger::init();

    let text = r#"
type Array a = { x : a }

type Foldable (f : Type -> Type) = {
    foldl : forall a b . (b -> a -> b) -> b -> f a -> b
}

let any x = any x

let foldable : Foldable Array =

    let foldl : forall a b . (a -> b -> b) -> b -> Array a -> b = any ()

    { foldl }
()
"#;
    let result = support::typecheck(text);

    assert_multi_unify_err!(
        result,
        [
            TypeMismatch(..),
            TypeMismatch(..),
            TypeMismatch(..),
            TypeMismatch(..)
        ]
    );
}

#[test]
fn issue_444() {
    let _ = ::env_logger::init();

    let text = r#"
let test x : () = () in 1
"#;
    let result = support::typecheck(text);

    assert_unify_err!(result, TypeMismatch(..));
}
