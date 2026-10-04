//! Version resolution and `PYENV_ROOT` layout, shared by the `pyenv` CLI and the shims.

pub mod console;
pub mod ctx;
pub mod debuglog;
pub mod flavor;
pub mod installed;
pub mod latest;
pub mod launch;
pub mod lookup;
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
pub mod verfile;
pub mod vsort;
pub mod wincmd;
#[cfg(windows)]
pub mod wincp;
#[cfg(windows)]
pub mod winproc;
pub mod winresolve;
