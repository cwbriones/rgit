// Delta encoding algorithm
use std::fs::File;
use std::io::Read;

use anyhow::{
    anyhow,
    Context,
    Result,
};
use byteorder::ReadBytesExt;

pub fn patch(source: &[u8], delta: &[u8]) -> Result<Vec<u8>> {
    let mut patcher = DeltaPatcher::new(source, delta);
    patcher.run_to_end()
}

pub fn patch_file(source_path: &str, delta_path: &str) -> Result<()> {
    let mut source_file = File::open(source_path)?;
    let mut source_contents = Vec::new();

    let mut delta_file = File::open(delta_path)?;
    let mut delta_contents = Vec::new();

    source_file.read_to_end(&mut source_contents)?;
    delta_file.read_to_end(&mut delta_contents)?;

    let mut patcher = DeltaPatcher::new(&source_contents[..], &delta_contents[..]);
    let res = patcher.run_to_end()?;
    // Git contents may not be valid utf-8 since a blob could be an arbitrary binary file.
    // Since this is for debugging only, print is best-effort.
    print!("{}", String::from_utf8_lossy(&res[..]));
    Ok(())
}

#[derive(Debug)]
enum DeltaOp {
    Insert(usize),
    Copy(usize, usize),
}

struct DeltaPatcher<'a> {
    source: &'a [u8],
    delta: &'a [u8],
}

impl<'a> DeltaPatcher<'a> {
    pub fn new(source: &'a [u8], delta: &'a [u8]) -> Self {
        DeltaPatcher { source, delta }
    }

    fn run_to_end(&mut self) -> Result<Vec<u8>> {
        let source_len = read_varint(&mut self.delta).with_context(|| "read source length")?;
        let target_len = read_varint(&mut self.delta).with_context(|| "read target length")?;
        if self.source.len() != source_len {
            return Err(anyhow!(
                "unexpected source length: got {}, expected {}",
                source_len,
                self.source.len()
            ));
        }
        let mut buf = Vec::with_capacity(target_len);
        while let Ok(command) = self.read_command() {
            self.run_command(command, &mut buf);
        }
        if buf.len() != target_len {
            return Err(anyhow!(
                "unexpected target length: got {}, expected {}",
                target_len,
                buf.len()
            ));
        }
        Ok(buf)
    }

    fn read_command(&mut self) -> Result<DeltaOp> {
        let cmd = self.delta.read_u8()?;
        if cmd & 128 == 0 {
            return Ok(DeltaOp::Insert(cmd as usize));
        }
        let mut offset = 0usize;
        let mut shift = 0usize;
        let mut length = 0usize;

        // Read the offset to copy from
        for mask in &[0x01, 0x02, 0x04, 0x08] {
            if cmd & mask > 0 {
                let byte = self.delta.read_u8()? as u64;
                offset += (byte as usize) << shift;
            }
            shift += 8;
        }

        // Read the length of the copy
        shift = 0;
        for mask in &[0x10, 0x20, 0x40] {
            if cmd & mask > 0 {
                let byte = self.delta.read_u8()? as u64;
                length += (byte << shift) as usize;
            }
            shift += 8;
        }
        if length == 0 {
            length = 0x10000;
        }
        Ok(DeltaOp::Copy(offset, length))
    }

    fn run_command(&mut self, command: DeltaOp, buf: &mut Vec<u8>) {
        match command {
            DeltaOp::Copy(start, length) => {
                buf.extend_from_slice(&self.source[start..start + length]);
            }
            DeltaOp::Insert(length) => {
                buf.extend_from_slice(&self.delta[..length]);
                self.delta = &self.delta[length..];
            }
        }
    }
}

fn read_varint<R: Read>(mut buf: R) -> Result<usize> {
    let mut byte = 0x80;
    let mut val = 0usize;
    let mut shift = 0usize;

    while (byte & 0x80) > 0 {
        byte = buf.read_u8()?;
        val += ((byte & 127) as usize) << shift;
        shift += 7;
    }
    Ok(val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_patching() {
        patch_file("tests/data/deltas/base1.txt", "tests/data/deltas/delta1").unwrap();
    }
}
