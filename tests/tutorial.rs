extern crate env_logger;
extern crate gluon;

use gluon::vm::api::generic::A;
use gluon::vm::api::{FunctionRef, Generic};
use gluon::RootedThread;
use gluon::import::Import;
use gluon::Compiler;

fn new_vm() -> RootedThread {
    let vm = ::gluon::new_vm();
    let import = vm.get_macros().get("import");
    import.as_ref()
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
        .run_expr::<Generic<A>>(&vm, "example", " import \"std/prelude.glu\" ")
        .unwrap();
    let mut add: FunctionRef<fn (i32, i32) -> i32> = vm.get_global("std.prelude.num_Int.(+)")
        .unwrap();
    let result = add.call(1, 2);
    assert_eq!(result, Ok(3));
}
#[test]
fn call_rust_from_gluon() {
    let _ = ::env_logger::init();
    fn factorial(x: i32) -> i32 {
        if x <= 1 { 1 } else { x * factorial(x - 1) }
    }
    let vm = new_vm();
    vm.define_global("factorial", factorial as fn (_) -> _)
        .unwrap();
    let result = Compiler::new()
        .run_expr::<i32>(&vm, "example", "factorial 5")
        .unwrap();
    assert_eq!(result, 120);
}

#[test]
fn use_string_module() {
    let _ = ::env_logger::init();
    let vm = new_vm();
    let result = Compiler::new()
        .run_expr::<String>(&vm, "example", " let string = import \"std/string.glu\" in string.trim \"  Hello world  \t\" ")
        .unwrap();
    assert_eq!(result, "Hello world");
}
