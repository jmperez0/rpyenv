use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // Run as `pyenv-<cmd>` (a built-in link, Decision 2): act as `pyenv <cmd>` (Decision 3).
    let mut args = args;
    if let Some(cmd) = std::env::args_os()
        .next()
        .and_then(|a| {
            std::path::Path::new(&a)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .and_then(|s| s.strip_prefix("pyenv-").map(str::to_string))
        .filter(|c| pyenv::is_builtin(Flavor::current(), c))
    {
        args.insert(0, cmd.into());
    }
    let output = match Ctx::from_process() {
        Ok(ctx) => pyenv::run(&args, &ctx),
        Err(e) => pyenv::Output::error(e.message()),
    };
    output.emit(Flavor::current());
    std::process::exit(output.code);
}
