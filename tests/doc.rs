extern crate gluon;

use gluon::{new_vm, Compiler};
use gluon::check::metadata::metadata;
use gluon::doc;

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
            values: vec![
                doc::Field {
                    name: "test",
                    typ: "forall a . a -> a".to_string(),
                    comment: "This is the test function",
                },
            ],
        }
    );
}
