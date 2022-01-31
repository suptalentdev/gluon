extern crate gluon_base as base;
#[macro_use]
extern crate gluon_vm as vm;
extern crate gluon;

#[macro_use]
extern crate collect_mac;
extern crate env_logger;
extern crate futures;
extern crate hyper;

use std::env;
use std::fmt;
use std::fs::File;
use std::io::{stderr, Read, Write};
use std::marker::PhantomData;
use std::str;
use std::sync::{Arc, Mutex};

use hyper::{Chunk, Method};
use hyper::status::StatusCode;
use hyper::server::Service;

use futures::future::{BoxFuture, Future};
use futures::stream::{BoxStream, Stream};

use base::types::{Type, ArcType};

use vm::{Result, Error as VmError};

use vm::thread::ThreadInternal;
use vm::thread::{Context, RootedThread, Thread};
use vm::Variants;
use vm::api::{VmType, Function, FunctionRef, FutureResult, Getable, OpaqueValue, Pushable,
              PushAsRef, IO, Userdata, ValueRef, WithVM};
use vm::gc::{Gc, Traverseable};

use vm::internal::Value;

use gluon::{Compiler, new_vm};

// `Handler` is a type defined in http.glu but since we need to refer to it in the signature of
// listen we define a phantom type to use with `OpaqueValue`
struct Handler<T>(PhantomData<T>);

impl<T: VmType + 'static> VmType for Handler<T> {
    type Type = Self;
    fn make_type(vm: &Thread) -> ArcType {
        let typ =
            (*vm.global_env().get_env().find_type_info("examples.http_types.Handler").unwrap())
                .clone()
                .into_type();
        Type::app(typ, collect![T::make_type(vm)])
    }
}

// Since we want to marshal types defined in hyper we use `Wrap` to implement the traits we need
struct Wrap<T>(T);

macro_rules! define_vmtype {
    ($name: ident) => {
        impl VmType for Wrap<$name> {
            type Type = $name;
            fn make_type(vm: &Thread) -> ArcType {
                let typ = concat!("examples.http_types.", stringify!($name));
                (*vm.global_env().get_env().find_type_info(typ).unwrap())
                    .clone()
                    .into_type()
            }
        }

    }
}

define_vmtype! { Method }

impl<'vm> Pushable<'vm> for Wrap<Method> {
    fn push(self, _: &'vm Thread, context: &mut Context) -> Result<()> {
        use hyper::Method::*;
        context.stack.push(Value::Tag(match self.0 {
            Get => 0,
            Post => 1,
            Delete => 2,
            _ => {
                return Err(VmError::Message(format!("Method `{:?}` does not exist in gluon",
                                                    self.0))
                    .into())
            }
        }));
        Ok(())
    }
}

define_vmtype! { StatusCode }

impl<'vm> Getable<'vm> for Wrap<StatusCode> {
    fn from_value(_: &'vm Thread, value: Variants) -> Option<Self> {
        use hyper::status::StatusCode::*;
        match value.as_ref() {
            ValueRef::Tag(tag) => {
                Some(Wrap(match tag {
                    0 => Ok,
                    1 => NotFound,
                    2 => InternalServerError,
                    _ => panic!("Unexpected tag"),
                }))
            }
            _ => panic!(),
        }
    }
}

pub struct Body(Arc<Mutex<BoxStream<PushAsRef<Chunk, [u8]>, VmError>>>);

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "hyper::Body")
    }
}

impl Userdata for Body {}

impl Traverseable for Body {
    fn traverse(&self, _: &mut Gc) {}
}

impl VmType for Body {
    type Type = Self;
}

fn read_chunk(body: &Body) -> FutureResult<BoxFuture<IO<Option<PushAsRef<Chunk, [u8]>>>, VmError>> {
    use futures::future::poll_fn;

    let body = body.0.clone();
    FutureResult(poll_fn(move || {
            let mut stream = body.lock().unwrap();
            stream.poll().map(|async| async.map(IO::Value))
        })
        .boxed())
}

field_decl! { method, uri, status, body  }

type Request = record_type!{
    method => Wrap<Method>,
    uri => String,
    body => Body
};
type Response = record_type!{
    status => Wrap<StatusCode>,
    body => String
};

fn listen(port: i32, value: WithVM<OpaqueValue<RootedThread, Handler<Response>>>) -> IO<()> {
    let WithVM { value: handler, vm: thread } = value;

    use hyper::Server;
    use hyper::server::Request as HyperRequest;
    use hyper::server::Response as HyperResponse;

    type ListenFn = fn(OpaqueValue<RootedThread, Handler<Response>>, Request) -> IO<Response>;
    let handle: Function<RootedThread, ListenFn> = thread.get_global("examples.http.handle")
        .unwrap_or_else(|err| panic!("{}", err));

    struct Listen {
        handle: Function<RootedThread, ListenFn>,
        handler: OpaqueValue<RootedThread, Handler<Response>>,
    }

    impl Service for Listen {
        type Request = HyperRequest;
        type Response = HyperResponse;
        type Error = hyper::Error;
        type Future = BoxFuture<HyperResponse, hyper::Error>;

        fn call(&self, request: HyperRequest) -> Self::Future {
            let gluon_request = record_no_decl! {
                method => Wrap(request.method().clone()),
                uri => request.uri().to_string(),
                body => Body(Arc::new(Mutex::new(request.body()
                    .map_err(|err| VmError::Message(format!("{}", err)))
                    // This makes the `body` parameter act as a `&[u8]` meaning it is marshalled
                    // into an `Array Byte` in gluon
                    .map(PushAsRef::<_, [u8]>::new)
                    .boxed())))
            };
            self.handle
                .clone()
                .call_async(self.handler.clone(), gluon_request)
                .then(|result| match result {
                    Ok(value) => {
                        match value {
                            IO::Value(record_p!{ status, body }) => {
                                Ok(HyperResponse::new()
                                    .with_status(status.0)
                                    .with_body(body))
                            }
                            IO::Exception(err) => {
                                let _ = stderr().write(err.as_bytes());
                                Ok(HyperResponse::new()
                                    .with_status(StatusCode::InternalServerError))
                            }
                        }
                    }
                    Err(err) => {
                        let _ = stderr().write(format!("{}", err).as_bytes());
                        Ok(HyperResponse::new().with_status(StatusCode::InternalServerError))
                    }
                })
                .boxed()
        }
    }

    let addr = format!("127.0.0.1:{}", port).parse().unwrap();
    let (_listening, server) = Server::standalone(move |tokio| {
            Server::http(&addr, tokio)
                ?
                .handle(move || {
                            Ok(Listen {
                                handle: handle.clone(),
                                handler: handler.clone(),
                            })
                        },
                        tokio)
        })
        .unwrap();
    server.run();

    IO::Value(())
}

pub fn load_types(vm: &Thread) -> Result<()> {
    vm.register_type::<Body>("Body", &[])?;
    Ok(())
}

pub fn load(vm: &Thread) -> Result<()> {
    vm.define_global("http_prim",
                       record! {
        listen => primitive!(2 listen),
        read_chunk => primitive!(1 read_chunk)
    })?;
    Ok(())
}

fn main() {
    let _ = env_logger::init();
    let port = env::args().nth(1).map(|port| port.parse::<i32>().expect("port")).unwrap_or(80);

    let mut expr = String::new();
    {
        let mut file = File::open("examples/http_server.glu").unwrap();
        file.read_to_string(&mut expr).unwrap();
    }
    let thread = new_vm();

    load_types(&thread).unwrap();
    Compiler::new()
        .run_expr::<()>(&thread,
                        "",
                        r#"let _ = import! "examples/http_types.glu" in () "#)
        .unwrap_or_else(|err| panic!("{}", err));
    load(&thread).unwrap();

    let (mut listen, _) = Compiler::new()
        .run_expr::<FunctionRef<fn(i32) -> IO<()>>>(&thread, "http_test", &expr)
        .unwrap_or_else(|err| panic!("{}", err));

    listen.call(port)
        .unwrap_or_else(|err| panic!("{}", err));
}
