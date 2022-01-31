#![feature(globs, phase, macro_rules, default_type_params, overloaded_calls)]
extern crate collections;
#[phase(plugin, link)]
extern crate log;

extern crate EmbedLang;

#[cfg(not(test))]
use EmbedLang::vm::{run_main, run_buffer_main};

mod repl;


#[cfg(not(test))]
fn main() {
    let args = ::std::os::args();
	println!("{}", args);
    if args.len() == 1 {
        let mut buffer = ::std::io::stdin();
        let (_, value) = run_buffer_main(&mut buffer)
            .unwrap_or_else(|err| fail!("{}", err));
        println!("{}", value);
    }
    else if args[1].as_slice() == "-i" {
        repl::run();
    }
    else if args.len() == 2 {
        println!("{}", run_main(args[1].as_slice()));
    }
}
