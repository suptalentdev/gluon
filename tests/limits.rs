extern crate env_logger;
extern crate gluon;

mod support;

use gluon::{Compiler, Error, Thread};
use gluon::vm::Error as VMError;
use gluon::vm::api::{Hole, OpaqueValue};
use gluon::vm::thread::ThreadInternal;

use support::make_vm;

#[test]
fn stack_overflow() {
    let _ = ::env_logger::init();
    let vm = make_vm();

    vm.context().set_stack_size_limit(3);

    let result = Compiler::new()
        .run_expr::<OpaqueValue<&Thread, Hole>>(&vm, "example", r#" [1, 2, 3, 4] "#);

    match result {
        Err(Error::VM(VMError::StackOverflow(3))) => (),
        Err(err) => panic!("Unexpected error `{:?}`", err),
        Ok(_) => panic!("Expected an error"),
    }
}
