use std::any::Any;
use std::fmt;
use std::sync::Mutex;
use std::marker::PhantomData;

use base::types::{Type, ArcType};
use base::fnv::FnvMap;
use Result;
use gc::{Gc, GcPtr, Move, Traverseable};
use vm::Thread;
use thread::ThreadInternal;
use value::{Value, deep_clone};
use api::{RuntimeResult, Generic, Userdata, VmType, WithVM};
use api::generic::A;

struct Reference<T> {
    value: Mutex<Value>,
    thread: GcPtr<Thread>,
    _marker: PhantomData<T>,
}

impl<T> Userdata for Reference<T>
    where T: Any + Send + Sync,
{
    fn deep_clone(&self,
                  visited: &mut FnvMap<*const (), Value>,
                  gc: &mut Gc,
                  thread: &Thread)
                  -> Result<GcPtr<Box<Userdata>>> {
        let value = self.value.lock().unwrap();
        let cloned_value = try!(deep_clone(*value, visited, gc, thread));
        let data: Box<Userdata> = Box::new(Reference {
            value: Mutex::new(cloned_value),
            thread: unsafe { GcPtr::from_raw(thread) },
            _marker: PhantomData::<A>,
        });
        gc.alloc(Move(data))
    }
}

impl<T> fmt::Debug for Reference<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Ref({:?})", *self.value.lock().unwrap())
    }
}

impl<T> Traverseable for Reference<T> {
    fn traverse(&self, gc: &mut Gc) {
        self.value.lock().unwrap().traverse(gc)
    }
}

impl<T> VmType for Reference<T>
    where T: VmType,
          T::Type: Sized,
{
    type Type = Reference<T::Type>;

    fn make_type(vm: &Thread) -> ArcType {
        let env = vm.global_env().get_env();
        let symbol = env.find_type_info("Ref").unwrap().name.clone();
        let ctor = Type::ident(symbol);
        Type::app(ctor, vec![T::make_type(vm)])
    }
}

fn set(r: &Reference<A>, a: Generic<A>) -> RuntimeResult<(), String> {
    match r.thread.deep_clone_value(a.0) {
        Ok(a) => {
            *r.value.lock().unwrap() = a;
            RuntimeResult::Return(())
        }
        Err(err) => RuntimeResult::Panic(format!("{}", err)),
    }
}

fn get(r: &Reference<A>) -> Generic<A> {
    Generic::from(*r.value.lock().unwrap())
}

fn make_ref(a: WithVM<Generic<A>>) -> Reference<A> {
    Reference {
        value: Mutex::new(a.value.0),
        thread: unsafe { GcPtr::from_raw(a.vm) },
        _marker: PhantomData,
    }
}

pub fn load(vm: &Thread) -> Result<()> {
    let _ = vm.register_type::<Reference<A>>("Ref", &["a"]);
    try!(vm.define_global("<-", primitive!(2 set)));
    try!(vm.define_global("load", primitive!(1 get)));
    try!(vm.define_global("ref", primitive!(1 make_ref)));
    Ok(())
}
