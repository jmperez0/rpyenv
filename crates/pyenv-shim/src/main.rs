//! The shim binary: `python`, `pip` and every other shim run this (spec §3, §5).

fn main() {
    std::process::exit(rpyenv_core::shim::main());
}
