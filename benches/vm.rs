#![feature(test)]

extern crate test;

extern crate parser;
extern crate check;
extern crate vm;

use std::fs::File;
use std::io::Read;

use check::typecheck::Typecheck;
use vm::vm::VM;

#[bench]
fn prelude(b: &mut ::test::Bencher) {
    let vm = VM::new();
    let env = vm.env();
    let mut interner = vm.interner.borrow_mut();
    let mut gc = vm.gc.borrow_mut();
    let mut text = String::new();
    File::open("std/prelude.hs").unwrap().read_to_string(&mut text).unwrap();
    let expr = ::parser::parse_tc(&mut *gc, &mut *interner, &text)
                   .unwrap_or_else(|err| panic!("{:?}", err));
    b.iter(|| {
        let mut tc = Typecheck::new(&mut *interner, &mut *gc);
        tc.add_environment(&env);
        let result = tc.typecheck_expr(&mut expr.clone());
        if let Err(ref err) = result {
            println!("{}", err);
            assert!(false);
        }
        ::test::black_box(result)
    })
}
