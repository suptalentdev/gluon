extern crate gluon;
extern crate gluon_doc as doc;

use gluon::check::metadata::metadata;
use gluon::{Compiler, RootedThread};

fn new_vm() -> RootedThread {
    ::gluon::VmBuilder::new()
        .import_paths(Some(vec!["..".into()]))
        .build()
}

#[test]
fn basic() {
    let vm = new_vm();
    let module = r#"
/// This is the test function
let test x = x
{ test }
"#;
    let (expr, typ) = Compiler::new()
        .typecheck_str(&vm, "basic", module, None)
        .unwrap();
    let (meta, _) = metadata(&*vm.get_env(), &expr);

    let out = doc::record(&typ, &meta);
    assert_eq!(
        out,
        doc::Record {
            types: Vec::new(),
            values: vec![doc::Field {
                name: "test".to_string(),
                args: vec![doc::Argument {
                    implicit: false,
                    name: "x".to_string(),
                }],
                typ: "forall a . a -> a".to_string(),
                comment: "This is the test function".to_string(),
            }],
        }
    );
}
