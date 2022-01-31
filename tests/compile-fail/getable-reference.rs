extern crate gluon;
use gluon::new_vm;
use gluon::vm::gc::{Gc, Traverseable};
use gluon::vm::api::{Pushable, VmType, Userdata};

#[derive(Debug)]
struct Test;

impl Userdata for Test {}

impl VmType for Test {
    type Type = Test;
}

impl Traverseable for Test {
    fn traverse(&self, _: &mut Gc) {}
}

fn f(_: &'static Test) {}

#[cfg_attr(rustfmt, rustfmt_skip)]
fn main() {
    let vm = new_vm();
    let f: fn(_) = f;
    vm.define_global("test", f);
    //~^ Error `vm` does not live long enough
}
