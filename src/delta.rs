// Delta encoding algorithm

use std::str as Str;
use std::io::Read;

#[derive(Debug)]
struct DeltaHeader {
    source_len: usize,
    target_len: usize,
    get_offset: usize,
}

impl DeltaHeader {
    fn new(delta: &[u8]) -> DeltaHeader {
        let (source, offset_p) = DeltaHeader::decode_size(delta, 0);
        let (target, offset) = DeltaHeader::decode_size(delta, offset_p);

        DeltaHeader {
            source_len: source,
            target_len: target,
            get_offset: offset
        }
    }

    fn decode_size(mut delta: &[u8], offset: usize) -> (usize, usize) {
        let _ = delta.read_exact(offset).unwrap();

        let mut byte = 0x80;
        let mut size = 0;
        let mut count = 0;

        while (byte & 0x80) > 0 {
            byte = delta.read_byte().unwrap() as usize;
            size += (byte & 127) << (7 * count);

            count += 1;
        }
        return (size, offset + count);
    }
}

#[derive(Debug)]
enum DeltaOp {
    Insert(usize),
    Copy(usize, usize)
}

pub fn patch_file(source: &str, delta: &str) {
    use std::fs::File;

    let mut source_file = File::open(source).unwrap();
    let mut source_contents = Vec::new();

    source_file.read_to_end(&mut source_contents);

    let mut delta_file = File::open(delta).unwrap();
    let mut delta_contents = Vec::new();

    delta_file.read_to_end(&mut delta_contents);

    let result = patch(&source_contents[..], &delta_contents[..]);
    println!("{}", Str::from_utf8(&result[..]).unwrap());
}

fn patch(source: &[u8], delta: &[u8]) -> Vec<u8> {
    let header = DeltaHeader::new(delta);
    println!("Header {:?}", header);

    if source.len() != header.source_len {
        panic!("source length and length specified by header do not match, expected {}, got {}", source.len(), header.source_len);
    }
    let mut result = Vec::with_capacity(header.target_len);
    run(header.get_offset, source, delta, &mut result);
    result
}

fn run(offset: usize, source: &[u8], mut delta: &[u8], buf: &mut Vec<u8>) {
    // Skip the first number of bytes
    let _ = delta.read_exact(offset).unwrap();
    while delta.len() > 0 {
        let command = read_command(&mut delta);
        run_command(command, source, &mut delta, buf);
    }
}

fn read_command(delta: &mut &[u8]) -> DeltaOp {
    let cmd = delta.read_byte().unwrap();

    if cmd & 128 > 0 {
        let mut offset = 0usize;
        let mut shift = 0usize;
        let mut length = 0usize;

        // Read the offset to copy from
        // for &mask in [0x01, 0x02, 0x04, 0x08].iter() {
        for &(i, mask) in [(1, 0x01), (2, 0x02), (3, 0x04), (4, 0x08)].iter() {
            if cmd & mask > 0 {
                let byte = delta.read_byte().unwrap() as u64;
                offset += (byte as usize) << shift;
            }
            shift += 8;
        }

        // Read the length of the copy
        shift = 0;
        // for &mask in [0x10, 0x20, 0x40].iter() {
        for &(i, mask) in [(5, 0x10), (6, 0x20), (7, 0x40)].iter() {
            println!("checking bit {}", i);
            if cmd & mask > 0 {
                let byte = delta.read_byte().unwrap() as u64;
                length += (byte << shift) as usize;
            }
            shift += 8;
        }
        DeltaOp::Copy(offset, length)
    } else {
        DeltaOp::Insert(cmd as usize)
    }
}

fn run_command(command: DeltaOp, source: &[u8], delta: &mut &[u8], buf: &mut Vec<u8>) {
    match command {
        DeltaOp::Copy(start, length) => {
            let end = start + length;
            buf.push_all(&source[start..end]);
        },
        DeltaOp::Insert(length) => {
            // TODO: Shouldn't have to do another allocation here in the read
            let items = delta.read_exact(length).unwrap();
            buf.push_all(&items[..]);
        }
    }
}

