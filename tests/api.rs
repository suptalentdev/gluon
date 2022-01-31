extern crate env_logger;
extern crate gluon;

use gluon::vm::api;
use gluon::vm::api::generic::A;
use gluon::vm::api::{Generic, VMType, FunctionRef};
use gluon::vm::thread::{RootedThread, Thread, Traverseable, Root, RootStr};
use gluon::vm::internal::Value;
use gluon::vm::types::VMInt;
use gluon::Compiler;
use gluon::import::Import;

fn load_script(vm: &Thread, filename: &str, input: &str) -> ::gluon::Result<()> {
    Compiler::new()
        .load_script(vm, filename, input)
}

fn run_expr(vm: &Thread, s: &str) -> Value {
    Compiler::new()
        .run_expr::<Generic<A>>(vm, "<top>", s)
        .unwrap_or_else(|err| panic!("{}", err))
        .0
}

fn make_vm() -> RootedThread {
    let vm = ::gluon::new_vm();
    let import = vm.get_macros().get("import");
    import.as_ref()
          .and_then(|import| import.downcast_ref::<Import>())
          .expect("Import macro")
          .add_path("..");
    vm
}

#[test]
fn call_function() {
    let _ = ::env_logger::init();
    let add10 = r"
let add10 : Int -> Int = \x -> x #Int+ 10 in add10
";
    let mul = r"
let mul : Float -> Float -> Float = \x y -> x #Float* y in mul
";
    let mut vm = make_vm();
    load_script(&mut vm, "add10", &add10).unwrap_or_else(|err| panic!("{}", err));
    load_script(&mut vm, "mul", &mul).unwrap_or_else(|err| panic!("{}", err));
    {
        let mut f: FunctionRef<fn(VMInt) -> VMInt> = vm.get_global("add10")
                                                    .unwrap();
        let result = f.call(2).unwrap();
        assert_eq!(result, 12);
    }
    let mut f: FunctionRef<fn(f64, f64) -> f64> = vm.get_global("mul").unwrap();
    let result = f.call(4., 5.).unwrap();
    assert_eq!(result, 20.);
}

#[test]
fn root_data() {
    let _ = ::env_logger::init();

    #[derive(Debug)]
    struct Test(VMInt);
    impl Traverseable for Test { }
    impl VMType for Test {
        type Type = Test;
    }

    let expr = r#"
\x -> test x 1
"#;
    let vm = make_vm();
    fn test(r: Root<Test>, i: VMInt) -> VMInt {
        r.0 + i
    }
    vm.register_type::<Test>("Test", &[])
      .unwrap_or_else(|_| panic!("Could not add type"));
    vm.define_global("test", {
          let test: fn(_, _) -> _ = test;
          test
      })
      .unwrap();
    load_script(&vm, "script_fn", expr).unwrap_or_else(|err| panic!("{}", err));
    let mut script_fn: FunctionRef<fn(api::Userdata<Test>) -> VMInt> = vm.get_global("script_fn").unwrap();
    let result = script_fn.call(api::Userdata(Test(123)))
                          .unwrap();
    assert_eq!(result, 124);
}

#[test]
fn root_string() {
    let _ = ::env_logger::init();
    let expr = r#"
test "hello"
"#;
    let mut vm = make_vm();
    fn test(s: RootStr) -> String {
        let mut result = String::from(&s[..]);
        result.push_str(" world");
        result
    }
    vm.define_global("test", {
          let test: fn(_) -> _ = test;
          test
      })
      .unwrap();
    let result = run_expr(&mut vm, expr);
    match result {
        Value::String(s) => assert_eq!(&s[..], "hello world"),
        x => panic!("Expected string {:?}", x),
    }
}
