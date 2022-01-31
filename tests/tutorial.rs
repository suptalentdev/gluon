extern crate env_logger;
#[macro_use]
extern crate gluon_vm;
extern crate gluon;

use gluon::base::types::Type;
use gluon::vm::api::{FunctionRef, Hole, OpaqueValue};
use gluon::{RootedThread, Thread};
use gluon::import::Import;
use gluon::Compiler;

fn new_vm() -> RootedThread {
    let vm = ::gluon::new_vm();
    let import = vm.get_macros().get("import");
    import
        .as_ref()
        .and_then(|import| import.downcast_ref::<Import>())
        .expect("Import macro")
        .add_path("..");
    vm
}

#[test]
fn access_field_through_alias() {
    let _ = ::env_logger::init();
    let vm = new_vm();
    Compiler::new()
        .run_expr_async::<OpaqueValue<&Thread, Hole>>(
            &vm,
            "example",
            r#" import! "std/prelude.glu" "#,
        )
        .sync_or_error()
        .unwrap();
    let mut add: FunctionRef<fn(i32, i32) -> i32> =
        vm.get_global("std.prelude.num_Int.(+)").unwrap();
    let result = add.call(1, 2);
    assert_eq!(result, Ok(3));
}

#[test]
fn call_rust_from_gluon() {
    let _ = ::env_logger::init();

    fn factorial(x: i32) -> i32 {
        if x <= 1 {
            1
        } else {
            x * factorial(x - 1)
        }
    }
    let vm = new_vm();
    vm.define_global("factorial", primitive!(1 factorial))
        .unwrap();

    let result = Compiler::new()
        .run_expr_async::<i32>(&vm, "example", "factorial 5")
        .sync_or_error()
        .unwrap();
    let expected = (120, Type::int());

    assert_eq!(result, expected);
}

#[test]
fn use_string_module() {
    let _ = ::env_logger::init();

    let vm = new_vm();
    let result = Compiler::new()
        .run_expr_async::<String>(
            &vm,
            "example",
            " let string  = import! \"std/string.glu\" in string.trim \"  \
             Hello world  \t\" ",
        )
        .sync_or_error()
        .unwrap();
    let expected = ("Hello world".to_string(), Type::string());

    assert_eq!(result, expected);
}
