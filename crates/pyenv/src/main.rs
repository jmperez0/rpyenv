use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let output = match Ctx::from_process() {
        Ok(ctx) => pyenv::run(&args, &ctx),
        Err(e) => pyenv::Output::error(e.message()),
    };
    output.emit(Flavor::current());
    std::process::exit(output.code);
}
