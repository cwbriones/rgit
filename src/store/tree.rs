use std::str;
use std::vec::Vec;

use nom::bytes::complete as bytes;
use nom::combinator::{
    map,
    map_res,
};
use nom::multi;
use nom::sequence as seq;

use crate::store::Sha;

#[derive(Debug)]
pub struct Tree {
    pub entries: Vec<TreeEntry>,
}

#[derive(Debug)]
pub struct TreeEntry {
    pub mode: EntryMode,
    pub path: String,
    pub sha: Sha,
}

#[derive(Debug, Clone)]
pub enum EntryMode {
    Normal,
    Executable,
    Symlink,
    Gitlink,
    SubDirectory,
}

impl Tree {
    pub fn parse(content: &[u8]) -> Option<Self> {
        if let Ok((_, entries)) = parse_tree(content) {
            Some(Tree { entries })
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct UnsupportedFileMode;

impl TryFrom<&[u8]> for EntryMode {
    type Error = UnsupportedFileMode;

    fn try_from(mode: &[u8]) -> Result<Self, Self::Error> {
        match mode {
            b"100644" | b"644" => Ok(EntryMode::Normal),
            b"100755" | b"755" => Ok(EntryMode::Executable),
            b"120000" => Ok(EntryMode::Symlink),
            b"160000" => Ok(EntryMode::Gitlink),
            b"40000" => Ok(EntryMode::SubDirectory),
            _ => Err(UnsupportedFileMode),
        }
    }
}

fn parse_tree(input: &[u8]) -> nom::IResult<&[u8], Vec<TreeEntry>> {
    multi::many1(parse_tree_entry)(input)
}

fn take_until_and_consume<T, I, E>(tag: T) -> impl Fn(I) -> nom::IResult<I, I, E>
where
    E: nom::error::ParseError<I>,
    I: nom::InputTake
        + nom::FindSubstring<T>
        + nom::Slice<std::ops::RangeFrom<usize>>
        + nom::InputIter<Item = u8>
        + nom::InputLength,
    T: nom::InputLength + Clone,
{
    use nom::bytes::complete::take;
    use nom::bytes::complete::take_until;
    use nom::sequence::terminated;

    move |input| terminated(take_until(tag.clone()), take(tag.input_len()))(input)
}

fn parse_tree_entry(input: &[u8]) -> nom::IResult<&[u8], TreeEntry> {
    let parts = seq::tuple((
        map_res(take_until_and_consume(" "), EntryMode::try_from),
        map_res(take_until_and_consume("\0"), str::from_utf8),
        map_res(bytes::take(20usize), Sha::from_bytes),
    ));
    map(parts, |(mode, path, sha)| TreeEntry {
        mode,
        path: path.to_string(),
        sha,
    })(input)
}

#[test]
fn test_parse_tree() {
    // The raw contents of a tree object.
    let input = [
        49, 48, 48, 54, 52, 52, 32, 46, 103, 105, 116, 105, 103, 110, 111, 114, 101, 0, 79, 255,
        178, 248, 156, 189, 143, 33, 105, 206, 153, 20, 189, 22, 189, 67, 120, 91, 179, 104, 49,
        48, 48, 54, 52, 52, 32, 67, 97, 114, 103, 111, 46, 116, 111, 109, 108, 0, 226, 11, 220, 57,
        33, 62, 223, 169, 46, 80, 98, 15, 155, 24, 209, 88, 234, 228, 138, 99, 49, 48, 48, 54, 52,
        52, 32, 82, 69, 65, 68, 77, 69, 46, 109, 100, 0, 189, 6, 31, 50, 207, 237, 81, 181, 168,
        222, 145, 109, 134, 186, 137, 235, 159, 208, 104, 242, 52, 48, 48, 48, 48, 32, 115, 114,
        99, 0, 44, 153, 32, 248, 175, 44, 114, 130, 179, 183, 191, 144, 34, 196, 7, 92, 15, 177,
        105, 86,
    ];
    println!("{:?}", String::from_utf8_lossy(&input));
    match parse_tree(&input) {
        Ok(_) => {}
        Err(e) => panic!("Failed to parse tree: {}", e),
    }
}
