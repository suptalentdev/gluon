use std::any::Any;
use std::marker::PhantomData;
use std::ops::Deref;

#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer};

use futures::{Async, Future};

use base::symbol::Symbol;
use base::types::ArcType;

use api::{ActiveThread, AsyncPushable, Getable, Pushable, RootedValue, VmType};
use compiler::{CompiledFunction, CompiledModule};
use future::FutureValue;
use gc::Move;
use stack::{ExternState, StackFrame};
use thread::{RootedThread, Status, Thread, ThreadInternal};
use types::{Instruction, VmIndex};
use value::{ExternFunction, Value, ValueRepr};
use {Error, Result, Variants};

pub type GluonFunction = extern "C" fn(&Thread) -> Status;

pub struct Primitive<F> {
    name: &'static str,
    function: GluonFunction,
    _typ: PhantomData<F>,
}

pub struct RefPrimitive<'vm, F> {
    name: &'static str,
    function: extern "C" fn(&'vm Thread) -> Status,
    _typ: PhantomData<F>,
}

#[inline]
pub fn primitive<F>(
    name: &'static str,
    function: extern "C" fn(&Thread) -> Status,
) -> Primitive<F> {
    Primitive {
        name: name,
        function: function,
        _typ: PhantomData,
    }
}

#[inline]
pub unsafe fn primitive_f<'vm, F>(
    name: &'static str,
    function: extern "C" fn(&'vm Thread) -> Status,
    _: F,
) -> RefPrimitive<'vm, F>
where
    F: VmFunction<'vm>,
{
    RefPrimitive {
        name: name,
        function: function,
        _typ: PhantomData,
    }
}

impl<'vm, F: VmType> VmType for Primitive<F> {
    type Type = F::Type;
    fn make_type(vm: &Thread) -> ArcType {
        F::make_type(vm)
    }
}

impl<'vm, F> Pushable<'vm> for Primitive<F>
where
    F: FunctionType + VmType,
{
    fn push(self, context: &mut ActiveThread<'vm>) -> Result<()> {
        let thread = context.thread();
        // Map rust modules into gluon modules
        let id = Symbol::from(self.name.replace("::", "."));
        let value = ValueRepr::Function(context.context().alloc_with(
            thread,
            Move(ExternFunction {
                id: id,
                args: F::arguments(),
                function: self.function,
            }),
        )?);
        context.push(value);
        Ok(())
    }
}

impl<'vm, F: VmType> VmType for RefPrimitive<'vm, F> {
    type Type = F::Type;
    fn make_type(vm: &Thread) -> ArcType {
        F::make_type(vm)
    }
}

impl<'vm, F> Pushable<'vm> for RefPrimitive<'vm, F>
where
    F: VmFunction<'vm> + FunctionType + VmType + 'vm,
{
    fn push(self, context: &mut ActiveThread<'vm>) -> Result<()> {
        use std::mem::transmute;
        let extern_function = unsafe {
            // The VM guarantess that it only ever calls this function with itself which should
            // make sure that ignoring the lifetime is safe
            transmute::<extern "C" fn(&'vm Thread) -> Status, extern "C" fn(&Thread) -> Status>(
                self.function,
            )
        };
        Primitive {
            function: extern_function,
            name: self.name,
            _typ: self._typ,
        }.push(context)
    }
}

pub struct CPrimitive {
    function: GluonFunction,
    args: VmIndex,
    id: Symbol,
}

impl CPrimitive {
    pub unsafe fn new(function: GluonFunction, args: VmIndex, id: &str) -> CPrimitive {
        CPrimitive {
            id: Symbol::from(id),
            function: function,
            args: args,
        }
    }
}

impl<'vm> Pushable<'vm> for CPrimitive {
    fn push(self, context: &mut ActiveThread<'vm>) -> Result<()> {
        use std::mem::transmute;

        let thread = context.thread();
        let function = self.function;
        let extern_function = unsafe {
            // The VM guarantess that it only ever calls this function with itself which should
            // make sure that ignoring the lifetime is safe
            transmute::<extern "C" fn(&'vm Thread) -> Status, extern "C" fn(&Thread) -> Status>(
                function,
            )
        };
        let value = context.context().alloc_with(
            thread,
            Move(ExternFunction {
                id: self.id,
                args: self.args,
                function: extern_function,
            }),
        )?;
        context.push(ValueRepr::Function(value));
        Ok(())
    }
}

fn make_type<T: ?Sized + VmType>(vm: &Thread) -> ArcType {
    <T as VmType>::make_type(vm)
}

/// Type which represents a function reference in gluon
pub type FunctionRef<'vm, F> = Function<&'vm Thread, F>;
pub type OwnedFunction<F> = Function<RootedThread, F>;

/// Type which represents an function in gluon
pub struct Function<T, F>
where
    T: Deref<Target = Thread>,
{
    value: RootedValue<T>,
    _marker: PhantomData<F>,
}

#[cfg(feature = "serde")]
impl<'de, V> Deserialize<'de> for Function<RootedThread, V> {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = ::api::de::deserialize_raw_value(deserializer)?;
        Ok(Function {
            value,
            _marker: PhantomData,
        })
    }
}

impl<T, F> Function<T, F>
where
    T: Deref<Target = Thread>,
{
    pub fn get_variant(&self) -> Variants {
        self.value.get_variant()
    }

    pub fn vm(&self) -> &Thread {
        self.value.vm()
    }
}

impl<T, F> Clone for Function<T, F>
where
    T: Deref<Target = Thread> + Clone,
{
    fn clone(&self) -> Self {
        Function {
            value: self.value.clone(),
            _marker: self._marker.clone(),
        }
    }
}

impl<T, F> VmType for Function<T, F>
where
    T: Deref<Target = Thread>,
    F: VmType,
{
    type Type = F::Type;
    fn make_type(vm: &Thread) -> ArcType {
        F::make_type(vm)
    }
}

impl<'vm, T, F: Any> Pushable<'vm> for Function<T, F>
where
    T: Deref<Target = Thread>,
    F: VmType,
{
    fn push(self, context: &mut ActiveThread<'vm>) -> Result<()> {
        context.push(self.value.get_variant());
        Ok(())
    }
}

impl<'vm, 'value, F> Getable<'vm, 'value> for Function<&'vm Thread, F> {
    fn from_value(vm: &'vm Thread, value: Variants<'value>) -> Function<&'vm Thread, F> {
        Function {
            value: vm.root_value(value.get_value()),
            _marker: PhantomData,
        } //TODO not type safe
    }
}

impl<'vm, 'value, F> Getable<'vm, 'value> for Function<RootedThread, F> {
    fn from_value(vm: &'vm Thread, value: Variants<'value>) -> Self {
        Function {
            value: vm.root_value(value.get_value()),
            _marker: PhantomData,
        } //TODO not type safe
    }
}

/// Trait which represents a function
pub trait FunctionType {
    /// Returns how many arguments the function needs to be provided to call it
    fn arguments() -> VmIndex;
}

/// Trait which abstracts over types which can be called by being pulling the arguments it needs
/// from the virtual machine's stack
pub trait VmFunction<'vm> {
    fn unpack_and_call(&self, vm: &'vm Thread) -> Status;
}

impl<'s, T: FunctionType> FunctionType for &'s T {
    fn arguments() -> VmIndex {
        T::arguments()
    }
}

impl<'vm, 's, T: ?Sized> VmFunction<'vm> for &'s T
where
    T: VmFunction<'vm>,
{
    fn unpack_and_call(&self, vm: &'vm Thread) -> Status {
        (**self).unpack_and_call(vm)
    }
}

impl<F> FunctionType for Box<F>
where
    F: FunctionType,
{
    fn arguments() -> VmIndex {
        F::arguments()
    }
}

impl<'vm, F: ?Sized> VmFunction<'vm> for Box<F>
where
    F: VmFunction<'vm>,
{
    fn unpack_and_call(&self, vm: &'vm Thread) -> Status {
        (**self).unpack_and_call(vm)
    }
}

macro_rules! vm_function_impl {
    ($f:tt, $($args:ident),*) => {

impl <'vm, $($args,)* R> VmFunction<'vm> for $f ($($args),*) -> R
where $($args: for<'value> Getable<'vm, 'value> + 'vm,)*
      R: AsyncPushable<'vm> + VmType + 'vm
{
    #[allow(non_snake_case, unused_mut, unused_assignments, unused_variables, unused_unsafe)]
    fn unpack_and_call(&self, vm: &'vm Thread) -> Status {
        let n_args = Self::arguments();
        let mut context = vm.current_context();
        let mut i = 0;
        let lock;
        let r = unsafe {
            let ($($args,)*) = {
                let stack = StackFrame::<ExternState>::current(context.stack());
                $(let $args = {
                    let x = $args::from_value_unsafe(vm, Variants::new(&stack[i]));
                    i += 1;
                    x
                });*;
// Lock the frame to ensure that any reference from_value_unsafe may have returned stay
// rooted
                lock = stack.into_lock();
                ($($args,)*)
            };
            drop(context);
            let r = (*self)($($args),*);
            context = vm.current_context();
            r
        };
        r.async_status_push(&mut context, lock)
    }
}

    }
}

macro_rules! make_vm_function {
    ($($args:ident),*) => (
impl <$($args: VmType,)* R: VmType> VmType for fn ($($args),*) -> R {
    #[allow(non_snake_case)]
    type Type = fn ($($args::Type),*) -> R::Type;

    #[allow(non_snake_case)]
    fn make_type(vm: &Thread) -> ArcType {
        let args = vec![$(make_type::<$args>(vm)),*];
        vm.global_env().type_cache().function(args, make_type::<R>(vm))
    }
}

vm_function_impl!(fn, $($args),*);
vm_function_impl!(Fn, $($args),*);

impl <'vm, $($args,)* R: VmType> FunctionType for fn ($($args),*) -> R {
    fn arguments() -> VmIndex {
        count!($($args),*) + R::extra_args()
    }
}

impl <'s, $($args,)* R: VmType> FunctionType for Fn($($args),*) -> R + 's {
    fn arguments() -> VmIndex {
        count!($($args),*) + R::extra_args()
    }
}

impl <'s, $($args: VmType,)* R: VmType> VmType for Fn($($args),*) -> R + 's {
    type Type = fn ($($args::Type),*) -> R::Type;

    #[allow(non_snake_case)]
    fn make_type(vm: &Thread) -> ArcType {
        <fn ($($args),*) -> R>::make_type(vm)
    }
}

impl<T, $($args,)* R> Function<T, fn($($args),*) -> R>
    where $($args: for<'vm> Pushable<'vm>,)*
          T: Deref<Target = Thread>,
          R: VmType + for<'x, 'value> Getable<'x, 'value>,
{
    #[allow(non_snake_case)]
    pub fn call(&mut self $(, $args: $args)*) -> Result<R> {
        match self.call_first($($args),*)? {
            Async::Ready(value) => Ok(value),
            Async::NotReady => Err(Error::Message("Unexpected async".into())),
        }
    }

    #[allow(non_snake_case)]
    fn call_first(&self $(, $args: $args)*) -> Result<Async<R>> {
        let vm = self.value.vm();
        let mut context = vm.current_context();
        context.push(self.value.get_variant());
        $(
            $args.push(&mut context)?;
        )*
        for _ in 0..R::extra_args() {
            0.push(&mut context).unwrap();
        }
        let args = count!($($args),*) + R::extra_args();
        match vm.call_function(context.into_owned(), args)? {
            Async::Ready(context) => {
                let value = context.unwrap().stack.pop();
                Self::return_value(vm, value).map(Async::Ready)
            }
            Async::NotReady => Ok(Async::NotReady),
        }
    }

    fn return_value(vm: &Thread, value: Value) -> Result<R> {
        unsafe {
            Ok(R::from_value(vm, Variants::new(&value)))
        }
    }
}

impl<T, $($args,)* R> Function<T, fn($($args),*) -> R>
    where $($args: for<'vm> Pushable<'vm>,)*
          T: Deref<Target = Thread> + Clone + Send,
          R: VmType + for<'x, 'value> Getable<'x, 'value> + Send + Sync + 'static,
{
    #[allow(non_snake_case)]
    pub fn call_async(
        &mut self
        $(, $args: $args)*
        ) -> Box<Future<Item = R, Error = Error> + Send + Sync + 'static>
    {
        use thread::Execute;
        use futures::IntoFuture;

        match self.call_first($($args),*) {
            Ok(ok) => {
                match ok {
                    Async::Ready(value) => Box::new(Ok(value).into_future()),
                    Async::NotReady => {
                        Box::new(
                            Execute::new(self.value.vm().root_thread())
                                .and_then(|(vm, value)| Self::return_value(&vm, value))
                        )
                    }
                }
            }
            Err(err) => {
                Box::new(Err(err).into_future())
            }
        }
    }

    #[allow(non_snake_case)]
    pub fn call_fast_async(
        &mut self
        $(, $args: $args)*
        ) -> FutureValue<Box<Future<Item = R, Error = Error> + Send + Sync + 'static>>
    {
        use thread::Execute;

        match self.call_first($($args),*) {
            Ok(ok) => {
                match ok {
                    Async::Ready(value) => FutureValue::Value(Ok(value)),
                    Async::NotReady => {
                        FutureValue::Future(Box::new(
                            Execute::new(self.value.vm().root_thread())
                                .and_then(|(vm, value)| Self::return_value(&vm, value))
                        ))
                    }
                }
            }
            Err(err) => {
                FutureValue::Value(Err(err))
            }
        }
    }
}
    )
}

make_vm_function!();
make_vm_function!(A);
make_vm_function!(A, B);
make_vm_function!(A, B, C);
make_vm_function!(A, B, C, D);
make_vm_function!(A, B, C, D, E);
make_vm_function!(A, B, C, D, E, F);
make_vm_function!(A, B, C, D, E, F, G);

pub struct TypedBytecode<T> {
    id: Symbol,
    args: VmIndex,
    instructions: Vec<Instruction>,
    _marker: PhantomData<T>,
}

impl<T> TypedBytecode<T> {
    pub fn new(name: &str, args: VmIndex, instructions: Vec<Instruction>) -> TypedBytecode<T>
    where
        T: VmType,
    {
        TypedBytecode {
            id: Symbol::from(name),
            args,
            instructions,
            _marker: PhantomData,
        }
    }
}

impl<T: VmType> VmType for TypedBytecode<T> {
    type Type = T::Type;

    fn make_type(vm: &Thread) -> ArcType {
        T::make_type(vm)
    }

    fn make_forall_type(vm: &Thread) -> ArcType {
        T::make_forall_type(vm)
    }

    fn extra_args() -> VmIndex {
        T::extra_args()
    }
}

impl<'vm, T: VmType> Pushable<'vm> for TypedBytecode<T> {
    fn push(self, context: &mut ActiveThread<'vm>) -> Result<()> {
        let thread = context.thread();
        let context = context.context();
        let mut compiled_module = CompiledModule::from(CompiledFunction::new(
            self.args,
            self.id,
            T::make_forall_type(thread),
            "".into(),
        ));
        compiled_module.function.instructions = self.instructions;
        let closure = thread.global_env().new_global_thunk(compiled_module)?;
        context.stack.push(ValueRepr::Closure(closure));
        Ok(())
    }
}
