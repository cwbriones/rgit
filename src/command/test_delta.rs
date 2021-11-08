use anyhow::{Context, Result};
use structopt::StructOpt;

use crate::delta;

#[derive(StructOpt)]
#[structopt(name = "test-delta", about = "reconstruct an object given a source and a delta")]
pub struct SubCommandTestDelta {
    source: String,
    delta: String,
}

impl SubCommandTestDelta {
    pub fn execute(&self) -> Result<()> {
        delta::patch_file(&self.source, &self.delta).context("patch file")
    }
}
