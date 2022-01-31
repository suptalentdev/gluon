
#[cfg(test)]
mod build {
    extern crate lalrpop;
    pub fn main() {
        lalrpop::Configuration::new()
            .process_current_dir()
            .unwrap();
        println!("cargo:rerun-if-changed=src/grammar.lalrpop");
    }
}

#[cfg(not(test))]
mod build {
    pub fn main() {}
}

fn main() {
    build::main();
}
