//! Version resolution and `PYENV_ROOT` layout, shared by the `pyenv` CLI and the shims.

pub mod ctx;
pub mod flavor;
pub mod installed;
pub mod latest;
pub mod lookup;
pub mod paths;
pub mod pathsearch;
pub mod prefix;
pub mod select;
pub mod shimset;
pub mod verfile;
pub mod vsort;
pub mod winresolve;
