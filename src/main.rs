use anyhow::Result;
use structopt::StructOpt;

mod command;
mod delta;
mod packfile;
mod remote;
mod store;

#[derive(StructOpt)]
#[structopt(about = "a toy git implementation in rust", version = env!("CARGO_PKG_VERSION"))]
#[structopt(flatten)]
enum Git {
    Clone(command::clone::SubcommandClone),
    CloneSsh(command::clone_ssh::SubCommandCloneSsh),
    ListRemote(command::ls_remote::ListRemote),
    ListRemoteSsh(command::ls_remote_ssh::SubcommandListRemoteSsh),
    Log(command::log::SubcommandLog),
    TestDelta(command::test_delta::SubCommandTestDelta),
}

fn main() -> Result<()> {
    let git = Git::from_args();
    match git {
        Git::Clone(c) => c.execute(),
        Git::CloneSsh(c) => c.execute(),
        Git::ListRemote(c) => c.execute(),
        Git::ListRemoteSsh(c) => c.execute(),
        Git::Log(c) => c.execute(),
        Git::TestDelta(c) => c.execute(),
    }
}
