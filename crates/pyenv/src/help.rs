//! Help texts, rendered exactly as each upstream prints them.

use crate::commands;
use crate::output::Output;
use rpyenv_core::flavor::Flavor;

struct Topic {
    name: &'static str,
    /// The `# Summary:` line. Commands without one are left out of the listing.
    summary: Option<&'static str>,
    /// What `pyenv help --usage <command>` prints.
    usage: Option<&'static str>,
    /// The full `pyenv help <command>` output.
    text: &'static str,
}

const fn topic(
    name: &'static str,
    summary: Option<&'static str>,
    usage: Option<&'static str>,
    text: &'static str,
) -> Topic {
    Topic {
        name,
        summary,
        usage,
        text,
    }
}

/// Upstream pyenv 2.8.6 (docs/parity/pyenv-m1-reference.md).
const PYENV: &[Topic] = &[
    topic("--version", Some("Display the version of pyenv"), None,
        "Usage: pyenv --version\n\nDisplays the version number of this pyenv release, including the\ncurrent revision from git, if available.\n\nThe format of the git revision is:\n  <version>-<num_commits>-<git_sha>\nwhere `num_commits` is the number of commits since `version` was\ntagged.\n\n"),
    topic("commands", Some("List all available pyenv commands"), Some("Usage: pyenv commands [--sh|--no-sh]"),
        "Usage: pyenv commands [--sh|--no-sh]\n\nList all available pyenv commands\n\n"),
    topic("exec", Some("Run an executable with the selected Python version"), Some("Usage: pyenv exec <command> [arg1 arg2...]"),
        "Usage: pyenv exec <command> [arg1 arg2...]\n\nRuns an executable by first preparing PATH so that the selected Python\nversion's `bin' directory is at the front.\n\nFor example, if the currently selected Python version is 2.7.6:\n  pyenv exec pip install -r requirements.txt\n\nis equivalent to:\n  PATH=\"$PYENV_ROOT/versions/2.7.6/bin:$PATH\" pip install -r requirements.txt\n\n"),
    topic("global", Some("Set or show the global Python version(s)"), Some("Usage: pyenv global <version> <version2> <..>"),
        "Usage: pyenv global <version> <version2> <..>\n\nSets the global Python version(s). You can override the global version at\nany time by setting a directory-specific version with `pyenv local'\nor by setting the `PYENV_VERSION' environment variable.\n\n<version> can be specified multiple times and should be a version\ntag known to pyenv.  The special version string `system' will use\nyour default system Python.  Run `pyenv versions' for a list of\navailable Python versions.\n\nExample: To enable the python2.7 and python3.7 shims to find their\n         respective executables you could set both versions with:\n\n'pyenv global 3.7.0 2.7.15'\n\n"),
    topic("help", Some("Display help for a command"), Some("Usage: pyenv help [--usage] COMMAND"),
        "Usage: pyenv help [--usage] COMMAND\n\nParses and displays help contents from a command's source file.\n\nA command is considered documented if it starts with a comment block\nthat has a `Summary:' or `Usage:' section. Usage instructions can\nspan multiple lines as long as subsequent lines are indented.\nThe remainder of the comment block is displayed as extended\ndocumentation.\n\n"),
    #[cfg(unix)]
    topic("install", Some("Install a Python version using python-build"), Some(commands::install::USAGE),
        commands::install::HELP),
    topic("latest", Some("Print the latest installed or known version with the given prefix"), Some("Usage: pyenv latest [-k|--known] <prefix>"),
        "Usage: pyenv latest [-k|--known] <prefix>\n\n  -k/--known      Select from all known versions instead of installed\n  -b/--bypass     (internal) On a resolution failure, do not print an error message\n                  but rather print the argument unchanged\n  -f/--force      (internal) Same as -b but also do not return a failure exit code\n\n"),
    topic("local", Some("Set or show the local application-specific Python version(s)"), Some("Usage: pyenv local [-f|--force] [<version> [...]]\n       pyenv local --unset"),
        "Usage: pyenv local [-f|--force] [<version> [...]]\n       pyenv local --unset\n\n  -f/--force    Do not verify that the versions being set exist\n\nSets the local application-specific Python version(s) by writing the\nversion name to a file named `.python-version'.\n\nWhen you run a Python command, pyenv will look for a `.python-version'\nfile in the current directory and each parent directory. If no such\nfile is found in the tree, pyenv will use the global Python version\nspecified with `pyenv global'. A version specified with the\n`PYENV_VERSION' environment variable takes precedence over local\nand global versions.\n\n<version> can be specified multiple times and should be a version\ntag known to pyenv.  The special version string `system' will use\nyour default system Python.  Run `pyenv versions' for a list of\navailable Python versions.\n\nExample: To enable the python2.7 and python3.7 shims to find their\n         respective executables you could set both versions with:\n\n'pyenv local 3.7.0 2.7.15'\n\n"),
    topic("prefix", Some("Display prefixes for Python versions"), Some("Usage: pyenv prefix [<version>...]"),
        "Usage: pyenv prefix [<version>...]\n\nDisplays the directories where the given Python versions are installed,\nseparated by colons. If no version is given, `pyenv prefix' displays the\nlocations of the currently selected versions.\n\n"),
    topic("rehash", Some("Rehash pyenv shims (run this after installing executables)"), None,
        "Usage: pyenv rehash\n\nRehash pyenv shims (run this after installing executables)\n\n"),
    topic("root", Some("Display the root directory where versions and shims are kept"), None,
        "Usage: pyenv root\n\nDisplay the root directory where versions and shims are kept\n\n"),
    topic("shims", Some("List existing pyenv shims"), Some("Usage: pyenv shims [--short]"),
        "Usage: pyenv shims [--short]\n\nList existing pyenv shims\n\n"),
    topic("version", Some("Show the current Python version(s) and its origin"), Some("Usage: pyenv version [--bare]"),
        "Usage: pyenv version [--bare]\n\n    --bare    show just the version name. An alias to `pyenv version-name'\n\n"),
    topic("version-file", Some("Detect the file that sets the current pyenv version"), Some("Usage: pyenv version-file [<dir>]"),
        "Usage: pyenv version-file [<dir>]\n\nDetect the file that sets the current pyenv version\n\n"),
    topic("version-file-read", None, Some("Usage: pyenv version-file-read <file>"),
        "Usage: pyenv version-file-read <file>\n"),
    topic("version-file-write", None, Some("Usage: pyenv version-file-write [-f|--force] <file> <version> [...]"),
        "Usage: pyenv version-file-write [-f|--force] <file> <version> [...]\n\n  -f/--force    Don't verify that the versions exist\n\n"),
    topic("version-name", Some("Show the current Python version"), None,
        "Usage: pyenv version-name\n\n  -f/--force    (Internal) If a version doesn't exist, print it as is rather than produce an error\n\n"),
    topic("version-origin", Some("Explain how the current Python version is set"), None,
        "Usage: pyenv version-origin\n\nExplain how the current Python version is set\n\n"),
    topic("versions", Some("List all Python versions available to pyenv"), Some("Usage: pyenv versions [--bare] [--skip-aliases] [--skip-envs] [--executables]"),
        "Usage: pyenv versions [--bare] [--skip-aliases] [--skip-envs] [--executables]\n\nLists all Python versions found in `$PYENV_ROOT/versions/*'.\n\n  --bare            List just the names, omit `system'\n  --skip-aliases    Skip symlinks to other versions and to virtual environments\n  --skip-envs       Skip virtual environments (under <version>/envs)\n  --executables     Internal. Overrides other options.\n                    Optimally get a deduplicated list of all executable names in Pyenv-managed\n                    versions and environments for `pyenv rehash'\n\n"),
    topic("whence", Some("List all Python versions that contain the given executable"), Some("Usage: pyenv whence [--path] <command>"),
        "Usage: pyenv whence [--path] <command>\n\nList all Python versions that contain the given executable\n\n"),
    topic("which", Some("Display the full path to an executable"), Some("Usage: pyenv which <command> [--nosystem] [--skip-advice]"),
        "Usage: pyenv which <command> [--nosystem] [--skip-advice]\n\nDisplays the full path to the executable that pyenv will invoke when\nyou run the given command.\nUse --nosystem argument in case when you don't need to search command in the \nsystem environment.\nInternal switch --skip-advice used to skip printing an error message on a\nfailed search.\n\n"),
];

/// pyenv-win at 856ed5a (docs/parity/pyenv-win-m1-reference.md), where pyenv-win has the command.
const PYENV_WIN: &[Topic] = &[
    topic("--version", None, None,
        "Usage: pyenv --version\n\nDisplays the version number of this pyenv release, including the\ncurrent revision from git, if available.\n\nThe format of the git revision is:\n  <major_version>-<train>-<minor_version>\nwhere `num_commits` is the number of commits since `minor_version` was\ntagged.\n\n"),
    topic("commands", None, None, "Usage: pyenv commands\n\nList all available pyenv commands\n\n"),
    topic("exec", None, None,
        "Usage: pyenv exec <command> [arg1 arg2...]\n\nRuns an executable by first preparing PATH so that the selected Python\nversion's `bin' directory is at the front.\n \nFor example, if the currently selected Python version is 3.5.3:\n  pyenv exec pip install -r requirements.txt\n \nis equivalent to:\n  PATH=\"$PYENV_ROOT/versions/3.5.3/bin:$PATH\" pip install -r requirements.txt\n\n"),
    topic("global", None, None,
        "Usage: pyenv global <version>\n       pyenv global --unset\n\nSets the global Python version. You can override the global version at\nany time by setting a directory-specific version with `pyenv local'\nor by setting the `PYENV_VERSION' environment variable.\n\n"),
    topic("local", None, None,
        "Usage: pyenv local <version> <version2> <..>\n       pyenv local --unset\n\nSets the local application-specific Python version by writing the\nversion name to a file named `.python-version'.\n\nWhen you run a Python command, pyenv will look for a `.python-version'\nfile in the current directory and each parent directory. If no such\nfile is found in the tree, pyenv will use the global Python version\nspecified with `pyenv global'. A version specified with the\n`PYENV_VERSION' environment variable takes precedence over local\nand global versions.\n\n<version> can be specified multiple times and should be a version\ntag known to pyenv.  The special version string `system' will use\nyour default system Python.  Run `pyenv versions' for a list of\navailable Python versions.\n\nExample: To enable the python2.7 and python3.7 shims to find their\n         respective executables you could set both versions with:\n\n'pyenv local 3.7.0 2.7.15'\n"),
    topic("rehash", None, None,
        "Usage: pyenv rehash\n\nRehash pyenv shims (run this after installing executables)\n\n"),
    topic("shims", None, None,
        "Usage: pyenv shims\n       pyenv shims --short\n\nList the existing pyenv shims\n\n"),
    topic("version", None, None,
        "Usage: pyenv version\n\nShows the currently selected Python version and how it was selected.\nTo obtain only the version string, use `pyenv vname' or `pyenv version-name`.\n"),
    topic("version-name", None, None, "Usage: pyenv version-name\n\nShows the currently selected Python version.\n"),
    topic("versions", None, None,
        "Usage: pyenv versions [--bare] [--skip-aliases]\n\nLists all Python versions found in `$PYENV_ROOT/versions/*'.\n"),
    topic("vname", None, None, "Usage: pyenv vname\n\nShows the currently selected Python version.\n"),
    topic("whence", None, None,
        "Usage: pyenv whence [--path] <command>\n\nShows the currently given executable contains path\nselected. To obtain python version of executable, use `pyenv whence pip'.\n"),
    topic("which", None, None,
        "Usage: pyenv which <command>\n\nShows the full path of the executable\nselected. To obtain the full path, use `pyenv which pip'.\n"),
];

/// pyenv-win's `pyenv help`, with the `)` that pyenv-win drops restored (allowlist D-08).
const WIN_HELP_LISTING: &str = "Usage: pyenv <command> [<args>]\n\nSome useful pyenv commands are:\n   commands    List all available pyenv commands\n   local       Set or show the local application-specific Python version\n   latest      Print the latest installed or known version with the given prefix\n   global      Set or show the global Python version\n   shell       Set or show the shell-specific Python version\n   install     Install a Python version using python-build\n   uninstall   Uninstall a specific Python version\n   rehash      Rehash pyenv shims (run this after installing executables)\n   version     Show the current Python version and its origin\n   versions    List all Python versions available to pyenv\n   which       Display the full path to an executable\n   whence      List all Python versions that contain the given executable\n\nSee `pyenv help <command>' for information on a specific command.\nFor full documentation, see: https://github.com/pyenv-win/pyenv-win#readme\n\n";

/// pyenv-win's `pyenv` with no arguments, after the version line and an empty line (allowlist D-20).
const WIN_SHOW_HELP: &str = "Usage: pyenv <command> [<args>]\n\nSome useful pyenv commands are:\n   commands     List all available pyenv commands\n   duplicate    Creates a duplicate python environment\n   local        Set or show the local application-specific Python version\n   latest       Print the latest installed or known version with the given prefix\n   global       Set or show the global Python version\n   shell        Set or show the shell-specific Python version\n   install      Install a Python version using python-build\n   uninstall    Uninstall a specific Python version\n   update       Update the cached version DB\n   rehash       Rehash pyenv shims (run this after installing executables)\n   vname        Show the current Python version\n   version      Show the current Python version and its origin\n   version-name Show the current Python version\n   versions     List all Python versions available to pyenv\n   exec         Runs an executable by first preparing PATH so that the selected Python\n   which        Display the full path to an executable\n   whence       List all Python versions that contain the given executable\n\nSee `pyenv help <command>' for information on a specific command.\nFor full documentation, see: https://github.com/pyenv-win/pyenv-win#readme\n";

/// The help topic for a command rpyenv has. On Windows, commands pyenv-win lacks
/// use upstream's text (allowlist D-12).
fn find(flavor: Flavor, name: &str) -> Option<&'static Topic> {
    commands::lookup(flavor, name)?;
    let win = match flavor {
        Flavor::PyenvWin => PYENV_WIN.iter().find(|t| t.name == name),
        Flavor::Pyenv => None,
    };
    win.or_else(|| PYENV.iter().find(|t| t.name == name))
}

/// Linux: upstream's list of the commands rpyenv has. Windows: pyenv-win's `pyenv help` text.
pub fn listing(flavor: Flavor) -> String {
    if flavor == Flavor::PyenvWin {
        return WIN_HELP_LISTING.to_string();
    }
    let mut topics: Vec<&Topic> = PYENV
        .iter()
        .filter(|t| t.summary.is_some() && commands::lookup(Flavor::Pyenv, t.name).is_some())
        .collect();
    topics.sort_by(|a, b| a.name.cmp(b.name));
    let mut s =
        String::from("Usage: pyenv <command> [<args>]\n\nSome useful pyenv commands are:\n");
    for t in topics {
        s.push_str(&format!(
            "   {:<9}   {}\n",
            t.name,
            t.summary.unwrap_or_default()
        ));
    }
    s.push_str("\nSee `pyenv help <command>' for information on a specific command.\n");
    s.push_str("For full documentation, see: https://github.com/pyenv/pyenv#readme\n");
    s
}

/// `pyenv help ...` for either flavor.
pub fn help_command(flavor: Flavor, args: &[&str]) -> Output {
    match flavor {
        Flavor::Pyenv => help_pyenv(args),
        Flavor::PyenvWin => help_win(args),
    }
}

/// Upstream `pyenv help [--usage] [<command>]`.
fn help_pyenv(args: &[&str]) -> Output {
    let mut o = Output::new();
    let (usage_only, rest) = match args.split_first() {
        Some((&"--usage", r)) => (true, r),
        _ => (false, args),
    };
    let cmd = match rest.first() {
        None | Some(&"pyenv") => None,
        Some(&c) => Some(c),
    };
    let Some(cmd) = cmd else {
        if usage_only {
            o.out("Usage: pyenv <command> [<args>]");
            return o.with_code(1);
        }
        o.stdout.push_str(&listing(Flavor::Pyenv));
        return o;
    };
    match find(Flavor::Pyenv, cmd) {
        Some(t) if usage_only => {
            if let Some(u) = t.usage {
                o.out(u);
            }
            o
        }
        Some(t) => {
            o.stdout.push_str(t.text);
            o
        }
        None => Output::error(format!("pyenv: no such command `{cmd}'")),
    }
}

/// pyenv-win `pyenv help [<command>]`; arguments after the command are ignored.
fn help_win(args: &[&str]) -> Output {
    let mut o = Output::new();
    let cmd = args.first().map(|c| c.to_ascii_lowercase());
    match cmd.as_deref() {
        None | Some("help" | "--help") => o.stdout.push_str(WIN_HELP_LISTING),
        Some(c) => match find(Flavor::PyenvWin, c) {
            Some(t) => o.stdout.push_str(t.text),
            None => {
                o.out(format!("pyenv: no such command '{}'", args[0]));
                return o.with_code(1);
            }
        },
    }
    o
}

/// pyenv-win's `pyenv` with no arguments.
pub fn show_help_win() -> Output {
    let mut o = Output::new();
    o.out(crate::version_line(Flavor::PyenvWin));
    o.out("");
    o.stdout.push_str(WIN_SHOW_HELP);
    o
}
