//! `pyenv sh-shell` and `pyenv sh-rehash` on Linux (libexec/pyenv-sh-shell and
//! pyenv-sh-rehash at 2.8.8; M3L). They print shell code for the `pyenv` function that
//! `pyenv init -` defines to evaluate. Windows' are in `shell_win.rs`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::shellname::{self, Family};
use rpyenv_core::{prefix, select};

/// `basename "${PYENV_SHELL:-$SHELL}"`, as the family the scripts tell apart: fish, pwsh,
/// or POSIX for every other name.
fn env_family(ctx: &Ctx) -> Family {
    let get = |k: &str| std::env::var(k).ok();
    let name = shellname::from_env(get("PYENV_SHELL").as_deref(), get("SHELL").as_deref());
    match shellname::family(&name, ctx.flavor) {
        Family::Fish => Family::Fish,
        Family::Pwsh => Family::Pwsh,
        _ => Family::Posix,
    }
}

const REVERT_POSIX: &str = r#"if [ -n "${PYENV_VERSION_OLD+x}" ]; then
  if [ -n "$PYENV_VERSION_OLD" ]; then
    PYENV_VERSION_OLD_="$PYENV_VERSION"
    export PYENV_VERSION="$PYENV_VERSION_OLD"
    PYENV_VERSION_OLD="$PYENV_VERSION_OLD_"
    unset PYENV_VERSION_OLD_
  else
    PYENV_VERSION_OLD="$PYENV_VERSION"
    unset PYENV_VERSION
  fi
else
  echo "pyenv: PYENV_VERSION_OLD is not set" >&2
  false
fi
"#;

const REVERT_FISH: &str = r#"if set -q PYENV_VERSION_OLD
  if [ -n "$PYENV_VERSION_OLD" ]
    set PYENV_VERSION_OLD_ "$PYENV_VERSION"
    set -gx PYENV_VERSION "$PYENV_VERSION_OLD"
    set -gu PYENV_VERSION_OLD "$PYENV_VERSION_OLD_"
    set -e PYENV_VERSION_OLD_
  else
    set -gu PYENV_VERSION_OLD "$PYENV_VERSION"
    set -e PYENV_VERSION
  end
else
  echo "pyenv: PYENV_VERSION_OLD is not set" >&2
  false
end
"#;

/// The line `} ` ends with a space, as upstream's heredoc does (libexec/pyenv-sh-shell:89).
const REVERT_PWSH: &str = "if ( Get-Item -Path Env:\\PYENV_VERSION* ) {\n  $Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = $Env:PYENV_VERSION_OLD, $Env:PYENV_VERSION\n} else {\n  Write-Error \"pyenv: Env:PYENV_VERSION_OLD is not set\"\n  return $false\n} \n";

/// `pyenv sh-shell [<version>...|-|--unset]`. Only the first argument is checked for
/// `--unset` and `-`; any other argument list is a list of versions.
pub fn sh_shell(ctx: &Ctx, args: &[&str]) -> Output {
    let family = env_family(ctx);
    let mut o = Output::new();
    match args.first().copied() {
        None | Some("") => match &ctx.pyenv_version {
            None => return Output::error("pyenv: no shell-specific version configured"),
            Some(_) => o.out("echo \"$PYENV_VERSION\""),
        },
        Some("--unset") => match family {
            Family::Fish => {
                o.out("set -gu PYENV_VERSION_OLD \"$PYENV_VERSION\"");
                o.out("set -e PYENV_VERSION");
            }
            Family::Pwsh => {
                o.out("$Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = $null, $Env:PYENV_VERSION")
            }
            _ => {
                o.out("PYENV_VERSION_OLD=\"${PYENV_VERSION-}\"");
                o.out("unset PYENV_VERSION");
            }
        },
        Some("-") => o.stdout.push_str(match family {
            Family::Fish => REVERT_FISH,
            Family::Pwsh => REVERT_PWSH,
            _ => REVERT_POSIX,
        }),
        Some(_) => {
            // `pyenv-prefix "${versions[@]}"` joins the arguments with `:`, splits them
            // again and resolves each: the first failure prints its message.
            let joined = args.join(":");
            for name in select::split_colon(&joined) {
                if let Err(e) = prefix::prefix_of(ctx, &name) {
                    o.err(e.message());
                    o.out("false");
                    return o.with_code(1);
                }
            }
            if ctx.pyenv_version.as_deref() != Some(joined.as_str()) {
                match family {
                    Family::Fish => {
                        o.out("set -gu PYENV_VERSION_OLD \"$PYENV_VERSION\"");
                        o.out(format!("set -gx PYENV_VERSION \"{joined}\""));
                    }
                    Family::Pwsh => o.out(format!(
                        "$Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = \"{joined}\", $Env:PYENV_VERSION"
                    )),
                    _ => {
                        o.out("PYENV_VERSION_OLD=\"${PYENV_VERSION-}\"");
                        o.out(format!("export PYENV_VERSION=\"{joined}\""));
                    }
                }
            }
        }
    }
    o
}

/// `pyenv sh-rehash`: the code that rehashes and, outside fish and pwsh, empties the
/// shell's command hash. It doesn't rehash itself; arguments are ignored.
pub fn sh_rehash(ctx: &Ctx, _args: &[&str]) -> Output {
    let mut o = Output::new();
    match env_family(ctx) {
        Family::Pwsh => o.out("& (get-command pyenv -commandtype application) rehash"),
        Family::Fish => o.out("command pyenv rehash"),
        _ => {
            o.out("command pyenv rehash");
            o.out("hash -r 2>/dev/null || true");
        }
    }
    o
}
