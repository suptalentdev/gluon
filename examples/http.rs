extern crate gluon_base as base;
#[macro_use]
extern crate gluon_vm as vm;
extern crate gluon;

#[macro_use]
extern crate collect_mac;
extern crate hyper;

use std::env;
use std::io::{stderr, Write};
use std::marker::PhantomData;

use self::hyper::method::Method;

use base::types::{Type, ArcType};

use vm::{Result, Error as VmError};

use vm::thread::ThreadInternal;
use vm::thread::{Context, RootedThread, Thread};
use vm::api::{VmType, Function, FunctionRef, OpaqueValue, Pushable, IO, WithVM};

use vm::internal::Value;

use gluon::{Compiler, new_vm};

// `Handler` is a type defined in http.glu but since we need to refer to it in the signature of
// listen we define a phantom type to use with `OpaqueValue`
struct Handler<T>(PhantomData<T>);

impl<T: VmType + 'static> VmType for Handler<T> {
    type Type = Self;
    fn make_type(vm: &Thread) -> ArcType {
        let typ = (*vm.global_env().get_env().find_type_info("examples.http_types.Handler").unwrap())
            .clone()
            .into_type();
        Type::app(typ, collect![T::make_type(vm)])
    }
}

// Since we want to marshal types defined in hyper we use `Wrap` to implement the traits we need
struct Wrap<T>(T);

impl VmType for Wrap<Method> {
    type Type = Method;
    fn make_type(vm: &Thread) -> ArcType {
        (*vm.global_env().get_env().find_type_info("examples.http_types.Method").unwrap())
            .clone()
            .into_type()
    }
}

impl<'vm> Pushable<'vm> for Wrap<Method> {
    fn push(self, _: &'vm Thread, context: &mut Context) -> Result<()> {
        use self::hyper::method::Method::*;
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

field_decl! { body, method }

type Request = record_type!( method => Wrap<Method> );
type Response = record_type!( body => String );

fn listen(port: i32, value: WithVM<OpaqueValue<RootedThread, Handler<Response>>>) -> IO<()> {
    let WithVM { value: handler, vm: thread } = value;

    use self::hyper::Server;
    use self::hyper::server::Request as HyperRequest;
    use self::hyper::server::Response as HyperResponse;

    let server = Server::http(("localhost", port as u16)).unwrap();
    type ListenFn = fn(OpaqueValue<RootedThread, Handler<Response>>, Request) -> IO<Response>;
    let handle: Function<RootedThread, ListenFn> = thread.get_global("examples.http.handle")
        .unwrap_or_else(|err| panic!("{}", err));
    let result = server.handle(move |request: HyperRequest, response: HyperResponse<_>| {
        let gluon_request = record_no_decl! {
            method => Wrap(request.method)
        };
        match handle.clone().call(handler.clone(), gluon_request).unwrap() {
            IO::Value(record_p!{ body }) => {
                response.send(body.as_bytes()).unwrap();
            }
            IO::Exception(ref err) => {
                let _ = stderr().write(err.as_bytes());
            }
        }
    });
    match result {
        Ok(_) => IO::Value(()),
        Err(err) => IO::Exception(err.to_string()),
    }
}

pub fn load(vm: &Thread) -> Result<()> {
    vm.define_global("http_prim",
                       record! {
        listen => primitive!(2 listen)
    })?;
    Ok(())
}

fn main() {
    let port = env::args().nth(1).map(|port| port.parse::<i32>().expect("port")).unwrap_or(80);

    let expr = r#"
        let prelude = import! "std/prelude.glu"
        let { show } = prelude.show_Int
        let string = import! "std/string.glu"
        let { (<>) } = prelude.make_Monoid string.monoid

        let { (*>) } = prelude.make_Applicative prelude.applicative_IO

        let { handle, get, applicative, listen } = import! "examples/http.glu"
        let { (*>), pure } = prelude.make_Applicative applicative

        let handler = get (pure { body = "Hello World" })

        \port ->
            io.println ("Opened server on port " <> show port) *>
                listen port handler
    "#;
    let thread = new_vm();

    Compiler::new()
        .run_expr::<()>(&thread, "", r#"let _ = import! "examples/http_types.glu" in () "#)
        .unwrap_or_else(|err| panic!("{}", err));
    load(&thread).unwrap();

    let (mut listen, _) = Compiler::new()
        .run_expr::<FunctionRef<fn(i32) -> IO<()>>>(&thread, "http_test", expr)
        .unwrap_or_else(|err| panic!("{}", err));

    listen.call(port)
        .unwrap_or_else(|err| panic!("{}", err));
}
