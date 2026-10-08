//! Version resolution and `PYENV_ROOT` layout, shared by the `pyenv` CLI and the shims.

#[cfg(windows)]
pub mod conpty;
pub mod console;
pub mod ctx;
pub mod debuglog;
pub mod flavor;
pub mod installed;
#[cfg(windows)]
pub mod junction;
pub mod latest;
pub mod launch;
pub mod livewatch;
#[cfg(target_os = "linux")]
pub mod livewatch_linux;
#[cfg(windows)]
pub mod livewatch_win;
pub mod lookup;
pub mod pathlist;
pub mod paths;
pub mod pathsearch;
pub mod pe;
pub mod plugins;
pub mod prefix;
pub mod rehash;
pub mod select;
pub mod shellname;
pub mod shim;
pub mod shimset;
pub mod textout;
pub mod venv;
pub mod verfile;
pub mod vsort;
pub mod vtscan;
pub mod wincmd;
#[cfg(windows)]
pub mod wincp;
#[cfg(windows)]
pub mod winenv;
#[cfg(windows)]
pub mod winproc;
pub mod winresolve;
