use std::any::Any;
use std::cell::Cell;
use std::marker::PhantomData;

use base::symbol::Symbol;
use base::types;
use base::types::{Type, TcType};
use gc::{Gc, GcPtr, Traverseable};
use stack::StackFrame;
use vm::{Thread, Value, Status};
use api::{MaybeError, Generic, Pushable, Userdata, VMType, WithVM};
use api::generic::A;

struct Reference<T> {
    value: Cell<Value>,
    thread: GcPtr<Thread>,
    _marker: PhantomData<T>,
}

impl<T> Traverseable for Reference<T> {
    fn traverse(&self, gc: &mut Gc) {
        self.value.traverse(gc)
    }
}

impl<T> VMType for Reference<T>
    where T: VMType,
          T::Type: Sized
{
    type Type = Reference<T::Type>;

    fn make_type(vm: &Thread) -> TcType {
        let env = vm.get_env();
        let symbol = env.find_type_info("Ref").unwrap().name.clone();
        let ctor = Type::id(symbol);
        Type::data(ctor, vec![T::make_type(vm)])
    }
}

impl<'vm, T> Pushable<'vm> for Reference<T>
    where T: Any + VMType,
          T::Type: Sized
{
    fn push<'b>(self, vm: &'vm Thread, stack: &mut StackFrame<'b>) -> Status {
        Userdata(self).push(vm, stack)
    }
}

fn set(r: &Reference<A>, a: Generic<A>) -> MaybeError<(), String> {
    match r.thread.deep_clone(a.0) {
        Ok(a) => {
            r.value.set(a);
            MaybeError::Ok(())
        }
        Err(err) => MaybeError::Err(format!("{}", err)),
    }
}

fn get(r: &Reference<A>) -> Generic<A> {
    Generic::from(r.value.get())
}

fn make_ref(a: WithVM<Generic<A>>) -> Reference<A> {
    Reference {
        value: Cell::new(a.value.0),
        thread: unsafe { GcPtr::from_raw(a.vm) },
        _marker: PhantomData,
    }
}

fn f1<A, R>(f: fn(A) -> R) -> fn(A) -> R {
    f
}
fn f2<A, B, R>(f: fn(A, B) -> R) -> fn(A, B) -> R {
    f
}

pub fn load(vm: &Thread) -> ::vm::Result<()> {
    let args = vec![types::Generic {
                        id: Symbol::new("a"),
                        kind: types::Kind::star(),
                    }];
    let _ = vm.register_type::<Reference<A>>("Ref", args.clone());
    try!(vm.define_global("<-", f2(set)));
    try!(vm.define_global("load", f1(get)));
    try!(vm.define_global("ref", f1(make_ref)));
    Ok(())
}
