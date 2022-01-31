#[macro_use]
extern crate criterion;

extern crate gluon;
extern crate gluon_base as base;
extern crate gluon_check as check;
extern crate gluon_parser as parser;

use std::fs;

use criterion::{Bencher, Criterion};

use gluon::compiler_pipeline::*;
use gluon::{new_vm, Compiler};

fn typecheck_prelude(b: &mut Bencher) {
    let vm = new_vm();
    let mut compiler = Compiler::new();
    let MacroValue { expr } = {
        let text = fs::read_to_string("std/prelude.glu").unwrap();
        text.expand_macro(&mut compiler, &vm, "std.prelude", &text)
            .unwrap_or_else(|(_, err)| panic!("{}", err))
    };
    b.iter(|| {
        let result = MacroValue { expr: expr.clone() }.typecheck(&mut compiler, &vm, "<top>", "");
        if let Err(ref err) = result {
            println!("{}", err);
            assert!(false);
        }
        result
    })
}

fn clone_prelude(b: &mut Bencher) {
    let vm = new_vm();
    let mut compiler = Compiler::new();
    let TypecheckValue { expr, .. } = {
        let text = fs::read_to_string("std/prelude.glu").unwrap();
        text.typecheck(&mut compiler, &vm, "std.prelude", &text)
            .unwrap_or_else(|err| panic!("{}", err))
    };
    b.iter(|| expr.clone())
}

fn typecheck_24(b: &mut Bencher) {
    let vm = new_vm();
    let mut compiler = Compiler::new();
    let MacroValue { expr } = {
        let text = fs::read_to_string("examples/24.glu").unwrap();
        text.expand_macro(&mut compiler, &vm, "examples.24", &text)
            .unwrap_or_else(|(_, err)| panic!("{}", err))
    };
    b.iter(|| {
        let result = MacroValue { expr: expr.clone() }.typecheck(&mut compiler, &vm, "<top>", "");
        if let Err(ref err) = result {
            println!("{}", err);
            assert!(false);
        }
        result
    })
}

fn clone_benchmark(c: &mut Criterion) {
    c.bench_function("clone prelude", clone_prelude);
}

fn typecheck_benchmark(c: &mut Criterion) {
    c.bench_function("std/prelude", typecheck_prelude);
    c.bench_function("examples/24", typecheck_24);
}

criterion_group!(check, typecheck_benchmark, clone_benchmark);
criterion_main!(check);
