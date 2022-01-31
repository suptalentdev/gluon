extern crate env_logger;
extern crate embed_lang;

use embed_lang::vm::api::{VMType, Function};

use embed_lang::vm::vm::{VM, VMInt, Value, Root, RootStr, load_script, run_expr};

fn make_vm<'a>() -> VM<'a> {
    let vm = VM::new();
    let import_symbol = vm.symbol("import");
    let import = vm.get_macros().get(import_symbol);
    import.as_ref()
          .and_then(|import| import.downcast_ref::<::embed_lang::vm::import::Import>())
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
        let mut f: Function<fn(VMInt) -> VMInt> = vm.get_global("add10")
                                                    .unwrap();
        let result = f.call(2).unwrap();
        assert_eq!(result, 12);
    }
    let mut f: Function<fn(f64, f64) -> f64> = vm.get_global("mul").unwrap();
    let result = f.call2(4., 5.).unwrap();
    assert_eq!(result, 20.);
}

#[test]
fn pass_userdata() {
    let _ = ::env_logger::init();
    let s = r"
let id : Test -> Test = \x -> x in id
";
    let mut vm = make_vm();
    fn test(test: *mut Test) -> bool {
        let test = unsafe { &mut *test };
        let x = test.x == 123;
        test.x *= 2;
        x
    }
    let test: fn(_) -> _ = test;
    impl VMType for Test {
        type Type = Test;
    }
    struct Test {
        x: VMInt,
    }
    vm.register_type::<Test>("Test", vec![])
      .unwrap_or_else(|_| panic!("Could not add type"));
    vm.define_global("test", test).unwrap_or_else(|err| panic!("{}", err));
    load_script(&mut vm, "id", &s).unwrap_or_else(|err| panic!("{}", err));

    let mut test = Test { x: 123 };
    {
        let mut f: Function<fn(*mut Test) -> *mut Test> = vm.get_global("id").unwrap();
        let result = f.call(&mut test).unwrap();
        let p: *mut _ = &mut test;
        assert!(result == p);
    }
    let mut f: Function<fn(*mut Test) -> bool> = vm.get_global("test").unwrap();
    let result = f.call(&mut test).unwrap();
    assert!(result);
    assert_eq!(test.x, 123 * 2);
}
#[test]
fn root_data() {
    let _ = ::env_logger::init();

    struct Test(VMInt);
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
    vm.register_type::<Test>("Test", vec![])
      .unwrap_or_else(|_| panic!("Could not add type"));
    vm.define_global("test", {
          let test: fn(_, _) -> _ = test;
          test
      })
      .unwrap();
    load_script(&vm, "script_fn", expr).unwrap_or_else(|err| panic!("{}", err));
    let mut script_fn: Function<fn(Box<Test>) -> VMInt> = vm.get_global("script_fn").unwrap();
    let result = script_fn.call(Box::new(Test(123)))
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
    let result = run_expr(&mut vm, "<top>", expr).unwrap_or_else(|err| panic!("{}", err));
    match *result {
        Value::String(s) => assert_eq!(&s[..], "hello world"),
        x => panic!("Expected string {:?}", x),
    }
}
