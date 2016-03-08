use clap::App;

pub mod clone;
pub mod clone_ssh;
pub mod ls_remote;
pub mod ls_remote_ssh;
pub mod log;
pub mod test_delta;

mod validators;

pub type SubCommand = App<'static, 'static, 'static, 'static, 'static, 'static>;
