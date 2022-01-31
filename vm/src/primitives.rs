use std::cell::Cell;
use std::io::Read;
use std::string::String as StdString;

use primitives as prim;
use api::{generic, Generic, Getable, Array, MaybeError, primitive};
use api::generic::A;
use gc::{Gc, Traverseable, DataDef, WriteOnly};
use vm::{VM, DataStruct, VMInt, Status, Value, RootStr, Result};


pub fn array_length(array: Array<generic::A>) -> VMInt {
    array.len() as VMInt
}

pub fn array_index<'a, 'vm>(array: Array<'a, 'vm, Generic<'a, generic::A>>,
                            index: VMInt)
                            -> MaybeError<Generic<'a, generic::A>, String> {
    match array.get(index) {
        Some(value) => MaybeError::Ok(value),
        None => MaybeError::Err(format!("{} is out of range", index)),
    }
}

pub fn array_append<'a, 'vm>(lhs: Array<'a, 'vm, Generic<'a, generic::A>>,
                             rhs: Array<'a, 'vm, Generic<'a, generic::A>>)
                             -> Array<'a, 'vm, Generic<'a, generic::A>> {
    struct Append<'a: 'b, 'b> {
        lhs: &'b [Cell<Value<'a>>],
        rhs: &'b [Cell<Value<'a>>],
    }

    impl<'a, 'b> Traverseable for Append<'a, 'b> {
        fn traverse(&self, gc: &mut Gc) {
            self.lhs.traverse(gc);
            self.rhs.traverse(gc);
        }
    }

    unsafe impl<'a, 'b> DataDef for Append<'a, 'b> {
        type Value = DataStruct<'a>;
        fn size(&self) -> usize {
            use std::mem::size_of;
            let len = self.lhs.len() + self.rhs.len();
            size_of::<usize>() + ::array::Array::<Value<'a>>::size_of(len)
        }
        fn initialize<'w>(self,
                          mut result: WriteOnly<'w, DataStruct<'a>>)
                          -> &'w mut DataStruct<'a> {
            unsafe {
                let result = &mut *result.as_mut_ptr();
                result.tag = 0;
                result.fields.initialize(self.lhs.iter().chain(self.rhs.iter()).cloned());
                result
            }
        }
    }
    let vm = lhs.vm();
    let value = {
        let stack = vm.current_frame();
        vm.alloc(&stack.stack,
                 Append {
                     lhs: &lhs.fields,
                     rhs: &rhs.fields,
                 })
    };
    Getable::from_value(lhs.vm(), Value::Data(value)).expect("Array")
}

pub fn string_length(s: RootStr) -> VMInt {
    s.len() as VMInt
}

pub fn string_append(l: RootStr, r: RootStr) -> String {
    let mut s = String::with_capacity(l.len() + r.len());
    s.push_str(&l);
    s.push_str(&r);
    s
}
pub fn string_eq(l: RootStr, r: RootStr) -> bool {
    *l == *r
}

pub fn string_compare(l: RootStr, r: RootStr) -> VMInt {
    use std::cmp::Ordering::*;
    match l.cmp(&r) {
        Less => -1,
        Equal => 0,
        Greater => 1,
    }
}
pub fn string_slice(s: RootStr, start: VMInt, end: VMInt) -> String {
    String::from(&s[start as usize..end as usize])
}
pub fn string_find(haystack: RootStr, needle: RootStr) -> Option<VMInt> {
    haystack.find(&needle[..]).map(|i| i as VMInt)
}
pub fn string_rfind(haystack: RootStr, needle: RootStr) -> Option<VMInt> {
    haystack.rfind(&needle[..]).map(|i| i as VMInt)
}
pub fn string_trim(s: RootStr) -> String {
    String::from(s.trim())
}

pub fn trace(vm: &VM) -> Status {
    let stack = vm.current_frame();
    println!("{:?}", stack[0]);
    Status::Ok
}

pub fn show_int(i: VMInt) -> String {
    format!("{}", i)
}

pub fn show_float(f: f64) -> String {
    format!("{}", f)
}

pub fn show_char(c: char) -> String {
    format!("{}", c)
}

pub fn error(_: &VM) -> Status {
    // We expect a string as an argument to this function but we only return Status::Error
    // and let the caller take care of printing the message
    Status::Error
}

fn f1<A, R>(f: fn(A) -> R) -> fn(A) -> R {
    f
}
fn f2<A, B, R>(f: fn(A, B) -> R) -> fn(A, B) -> R {
    f
}
fn f3<A, B, C, R>(f: fn(A, B, C) -> R) -> fn(A, B, C) -> R {
    f
}
pub fn load(vm: &VM) -> Result<()> {
    try!(vm.define_global("array",
                          record!(
        length => f1(prim::array_length),
        index => f2(prim::array_index),
        append => f2(prim::array_append)
    )));

    try!(vm.define_global("string_prim",
                          record!(
        length => f1(prim::string_length),
        find => f2(prim::string_find),
        rfind => f2(prim::string_rfind),
        trim => f1(prim::string_trim),
        compare => f2(prim::string_compare),
        append => f2(prim::string_append),
        eq => f2(prim::string_eq),
        slice => f3(prim::string_slice)
    )));
    try!(vm.define_global("prim",
                          record!(
        show_Int => f1(prim::show_int),
        show_Float => f1(prim::show_float),
        show_Char => f1(prim::show_char)
    )));

    try!(vm.define_global("#error",
                          primitive::<fn(StdString) -> A>("#error", prim::error)));
    try!(vm.define_global("error",
                          primitive::<fn(StdString) -> A>("error", prim::error)));
    try!(vm.define_global("trace", primitive::<fn(A)>("trace", prim::trace)));

    try!(::lazy::load(vm));
    Ok(())
}
