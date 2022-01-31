use std::any::Any;
use std::collections::VecDeque;
use std::fmt;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use futures::{
    future::{self, Either},
    Future,
};

use base::types::{ArcType, Type};

use api::generic::{A, B};
use api::{
    primitive, AsyncPushable, Function, FunctionRef, FutureResult, Generic, Getable, OpaqueRef,
    OpaqueValue, OwnedFunction, Pushable, Pushed, RuntimeResult, Unrooted, VmType, WithVM, IO,
};
use gc::{Gc, GcPtr, Traverseable};
use stack::{ClosureState, ExternState, StackFrame, State};
use thread::{ActiveThread, ThreadInternal};
use types::{VmIndex, VmInt};
use value::{Callable, GcStr, Userdata, Value, ValueRepr};
use vm::{RootedThread, Status, Thread};
use {Error, ExternModule, Result as VmResult};

pub struct Sender<T> {
    // No need to traverse this thread reference as any thread having a reference to this `Sender`
    // would also directly own a reference to the `Thread`
    thread: GcPtr<Thread>,
    queue: Arc<Mutex<VecDeque<Value>>>,
    _element_type: PhantomData<T>,
}

impl<T> Userdata for Sender<T> where T: Any + Send + Sync + fmt::Debug {}

impl<T> fmt::Debug for Sender<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", *self.queue.lock().unwrap())
    }
}

impl<T> Traverseable for Sender<T> {
    fn traverse(&self, _gc: &mut Gc) {
        // No need to traverse in Sender as values can only be accessed through Receiver
    }
}

impl<T> Sender<T> {
    fn send(&self, value: Value) {
        self.queue.lock().unwrap().push_back(value);
    }
}

impl<T> Traverseable for Receiver<T> {
    fn traverse(&self, gc: &mut Gc) {
        self.queue.lock().unwrap().traverse(gc);
    }
}

pub struct Receiver<T> {
    queue: Arc<Mutex<VecDeque<Value>>>,
    _element_type: PhantomData<T>,
}

impl<T> Userdata for Receiver<T> where T: Any + Send + Sync + fmt::Debug {}

impl<T> fmt::Debug for Receiver<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", *self.queue.lock().unwrap())
    }
}

impl<T> Receiver<T> {
    fn try_recv(&self) -> Result<Value, ()> {
        self.queue.lock().unwrap().pop_front().ok_or(())
    }
}

impl<T: VmType> VmType for Sender<T>
where
    T::Type: Sized,
{
    type Type = Sender<T::Type>;
    fn make_type(vm: &Thread) -> ArcType {
        let symbol = vm
            .global_env()
            .get_env()
            .find_type_info("Sender")
            .unwrap()
            .name
            .clone();
        Type::app(Type::ident(symbol), collect![T::make_type(vm)])
    }
}

impl<T: VmType> VmType for Receiver<T>
where
    T::Type: Sized,
{
    type Type = Receiver<T::Type>;
    fn make_type(vm: &Thread) -> ArcType {
        let symbol = vm
            .global_env()
            .get_env()
            .find_type_info("Receiver")
            .unwrap()
            .name
            .clone();
        Type::app(Type::ident(symbol), collect![T::make_type(vm)])
    }
}

field_decl!{ sender, receiver }

pub type ChannelRecord<S, R> = record_type!(sender => S, receiver => R);

/// FIXME The dummy `a` argument should not be needed to ensure that the channel can only be used
/// with a single type
fn channel(WithVM { vm, .. }: WithVM<Generic<A>>) -> ChannelRecord<Sender<A>, Receiver<A>> {
    let sender = Sender {
        thread: unsafe { GcPtr::from_raw(vm) },
        queue: Arc::new(Mutex::new(VecDeque::new())),
        _element_type: PhantomData,
    };
    let receiver = Receiver {
        queue: sender.queue.clone(),
        _element_type: PhantomData,
    };
    record_no_decl!(sender => sender, receiver => receiver)
}

fn recv(receiver: &Receiver<A>) -> Result<Unrooted<A>, ()> {
    receiver.try_recv().map_err(|_| ()).map(Unrooted::from)
}

fn send(sender: &Sender<A>, value: Generic<A>) -> Result<(), ()> {
    let value = sender
        .thread
        .deep_clone_value(&sender.thread, value.get_variant())
        .map_err(|_| ())?;
    Ok(sender.send(value))
}

extern "C" fn resume(vm: &Thread) -> Status {
    let mut context = vm.current_context();
    let value = StackFrame::<ExternState>::current(context.stack())[0].get_repr();
    match value {
        ValueRepr::Thread(child) => {
            let lock = StackFrame::<ExternState>::current(context.stack()).into_lock();
            drop(context);
            let result = child.resume();
            context = vm.current_context();
            context.stack().release_lock(lock);
            match result {
                Ok(child_context) => {
                    // Prevent dead lock if the following status_push call allocates
                    drop(child_context);

                    let value: Result<(), &str> = Ok(());
                    value.status_push(&mut context)
                }
                Err(Error::Dead) => {
                    let value: Result<(), &str> = Err("Attempted to resume a dead thread");
                    value.status_push(&mut context)
                }
                Err(err) => {
                    let fmt = format!("{}", err);
                    let result = unsafe {
                        ValueRepr::String(GcStr::from_utf8_unchecked(
                            context.context().alloc_ignore_limit(fmt.as_bytes()),
                        ))
                    };
                    context.push(result);
                    Status::Error
                }
            }
        }
        _ => unreachable!(),
    }
}

extern "C" fn yield_(_vm: &Thread) -> Status {
    Status::Yield
}

fn spawn<'vm>(
    value: WithVM<'vm, Function<&'vm Thread, fn(())>>,
) -> RuntimeResult<RootedThread, Error> {
    spawn_(value).into()
}
fn spawn_<'vm>(value: WithVM<'vm, Function<&'vm Thread, fn(())>>) -> VmResult<RootedThread> {
    let thread = value.vm.new_thread()?;
    {
        let mut context = thread.current_context();
        let callable = match value.value.get_variant().0 {
            ValueRepr::Closure(closure) => State::Closure(ClosureState {
                closure,
                instruction_index: 0,
            }),
            ValueRepr::Function(function) => State::Extern(ExternState::new(function)),
            _ => State::Unknown,
        };
        value.value.push(&mut context)?;
        context.push(ValueRepr::Int(0));
        StackFrame::<State>::current(&mut context.context().stack).enter_scope(1, callable);
    }
    Ok(thread)
}

type Action<T> = fn(()) -> OpaqueValue<RootedThread, IO<Pushed<T>>>;

#[cfg(target_arch = "wasm32")]
fn spawn_on<'vm>(
    _thread: RootedThread,
    _action: WithVM<'vm, FunctionRef<Action<A>>>,
) -> IO<OpaqueValue<&'vm Thread, IO<A>>> {
    IO::Exception("spawn_on requires the `tokio` crate".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_on<'vm>(
    thread: RootedThread,
    action: WithVM<'vm, FunctionRef<Action<A>>>,
) -> IO<Pushed<IO<A>>> {
    struct SpawnFuture<F>(future::Shared<F>)
    where
        F: Future;

    impl<F> fmt::Debug for SpawnFuture<F>
    where
        F: Future,
    {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "Future")
        }
    }

    impl<F> Userdata for SpawnFuture<F>
    where
        F: Future + Send + 'static,
        F::Item: Send + Sync,
        F::Error: Send + Sync,
    {
    }

    impl<F> Traverseable for SpawnFuture<F>
    where
        F: Future,
    {
        fn traverse(&self, _: &mut Gc) {}
    }

    impl<F> VmType for SpawnFuture<F>
    where
        F: Future,
        F::Item: VmType,
    {
        type Type = <F::Item as VmType>::Type;
    }

    fn push_future_wrapper<G>(context: &mut ActiveThread, _: &G)
    where
        G: Future<Item = OpaqueValue<RootedThread, IO<Pushed<A>>>, Error = Error> + Send + 'static,
    {
        extern "C" fn future_wrapper<F>(vm: &Thread) -> Status
        where
            F: Future<Item = OpaqueValue<RootedThread, IO<Pushed<A>>>, Error = Error>
                + Send
                + 'static,
        {
            let mut context = vm.current_context();
            let value = StackFrame::<ExternState>::current(context.stack())[0].get_repr();

            match value {
                ValueRepr::Userdata(data) => {
                    let data = data.downcast_ref::<SpawnFuture<F>>().unwrap();
                    let future = data.0.clone();
                    let frame_index = context.stack().get_frames().len() as VmIndex - 1;
                    AsyncPushable::async_status_push(
                        FutureResult::new(
                            future.map(|v| (*v).clone()).map_err(|err| (*err).clone()),
                        ),
                        &mut context,
                        frame_index,
                    )
                }
                _ => unreachable!(),
            }
        }

        type FutureArg = ();
        primitive::<fn(FutureArg) -> IO<Pushed<A>>>("unknown", future_wrapper::<G>)
            .push(context)
            .unwrap();
    }
    use value::PartialApplicationDataDef;

    let WithVM { vm, value: action } = action;
    let mut action = OwnedFunction::<Action<A>>::from_value(&thread, action.get_variant());

    let future = future::lazy(move || action.call_async(()));

    let mut context = vm.current_context();

    push_future_wrapper(&mut context, &future);

    let callable = match context.stack()[..].last().unwrap().get_repr() {
        ValueRepr::Function(ext) => Callable::Extern(ext),
        _ => unreachable!(),
    };

    SpawnFuture(future.shared()).push(&mut context).unwrap();
    let context = context.context();
    let fields = [context.stack.get_values()[..].last().unwrap().clone()];
    let def = PartialApplicationDataDef(callable, &fields);
    let value = ValueRepr::PartialApplication(context.alloc_with(vm, def).unwrap());

    context.stack.pop_many(2);
    context.stack.push(value);

    IO::Value(Pushed::default())
}

fn join(
    WithVM { vm: vm_a, value: a }: WithVM<OpaqueRef<IO<A>>>,
    b: OpaqueRef<IO<B>>,
) -> impl Future<Item = IO<(Generic<A>, Generic<B>)>, Error = Error> {
    let vm_b = try_future!(vm_a.new_thread(), Either::B);

    let mut action_a: OwnedFunction<fn(()) -> OpaqueValue<RootedThread, A>> =
        Getable::from_value(&vm_a, a.get_variant());
    let mut action_b: OwnedFunction<fn(()) -> OpaqueValue<RootedThread, B>> =
        Getable::from_value(&vm_b, b.get_variant());

    Either::A(
        action_a
            .call_fast_async(())
            .join(action_b.call_fast_async(()))
            .then(|result| {
                trace!("join done: {:?}", result);
                result
            })
            .map(IO::Value),
    )
}

fn new_thread(WithVM { vm, .. }: WithVM<()>) -> IO<RootedThread> {
    match vm.new_thread() {
        Ok(thread) => IO::Value(thread),
        Err(err) => IO::Exception(err.to_string()),
    }
}

fn sleep(ms: VmInt) -> IO<()> {
    use std::time::Duration;
    ::std::thread::sleep(Duration::from_millis(ms as u64));
    IO::Value(())
}

fn interrupt(thread: RootedThread) -> IO<()> {
    thread.interrupt();
    IO::Value(())
}

mod std {
    pub use channel;
    pub mod thread {
        pub use channel as prim;
    }
}

pub fn load_channel<'vm>(vm: &'vm Thread) -> VmResult<ExternModule> {
    let _ = vm.register_type::<Sender<A>>("Sender", &["a"]);
    let _ = vm.register_type::<Receiver<A>>("Receiver", &["a"]);

    ExternModule::new(
        vm,
        record!{
            type Sender a => Sender<A>,
            type Receiver a => Sender<A>,
            channel => primitive!(1, std::channel::channel),
            recv => primitive!(1, std::channel::recv),
            send => primitive!(2, std::channel::send),
        },
    )
}

pub fn load_thread<'vm>(vm: &'vm Thread) -> VmResult<ExternModule> {
    ExternModule::new(
        vm,
        record!{
            resume => primitive::<fn(&'vm Thread) -> Result<(), String>>("std.thread.prim.resume", resume),
            (yield_ "yield") => primitive::<fn(())>("std.thread.prim.yield", yield_),
            spawn => primitive!(1, std::thread::prim::spawn),
            spawn_on => primitive!(2, std::thread::prim::spawn_on),
            new_thread => primitive!(1, std::thread::prim::new_thread),
            interrupt => primitive!(1, std::thread::prim::interrupt),
            sleep => primitive!(1, std::thread::prim::sleep),
            join => primitive!(2, async fn std::thread::prim::join),
        },
    )
}
