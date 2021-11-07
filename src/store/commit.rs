use std::str::{self, FromStr};
use std::fmt::{self, Display, Formatter};

use nom::{
    IResult,
    alt,
    alt_parser,
    call,
    chain,
    chaining_parser,
    char,
    digit,
    line_ending,
    many0,
    map,
    map_impl,
    map_res,
    map_res_impl,
    named,
    newline,
    rest,
    space,
    tag,
    tag_bytes,
    take,
    take_until_and_consume,
    take_until_and_consume_bytes,
};
use chrono::naive::NaiveDateTime;
use chrono::DateTime;
use chrono::offset::FixedOffset;
use faster_hex::{
    hex_decode,
    hex_string,
};

use crate::store::GitObject;

pub struct Person<'a> {
    name: &'a str,
    email: &'a str,
    timestamp: DateTime<FixedOffset>
}

pub struct Commit<'a> {
    pub tree: Vec<u8>,
    pub parents: Vec<Vec<u8>>,
    author: Person<'a>,
    #[allow(dead_code)]
    committer: Person<'a>,
    message: &'a str,
    raw: &'a GitObject
}

impl<'a> Commit<'a> {
    pub fn from_raw(raw: &'a GitObject) -> Option<Self> {
        if let IResult::Done(_, raw_parts) = parse_commit_inner(&raw.content) {
            let (tree, parents, author, committer, message) = raw_parts;
            Some(Commit {
                tree,
                parents,
                author,
                committer,
                message,
                raw,
            })
        } else {
            None
        }
    }
}

impl<'a> Display for Person<'a> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        writeln!(f, "Author: {} <{}>", self.name, self.email)?;
        let formatted = self.timestamp.format("%a %b %-e %T %Y %z");
        writeln!(f, "Date:   {}", formatted)?;
        Ok(())
    }
}

impl<'a> Display for Commit<'a> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        writeln!(f, "commit {}", hex_string(&self.raw.sha()[..]))?;
        write!(f, "{}", self.author)?;
        for line in self.message.split('\n') {
            write!(f, "\n    {}", line)?;
        }
        Ok(())
    }
}

named!(u64_digit<u64>,
    map_res!(
        map_res!(
            digit,
            str::from_utf8
        ),
    FromStr::from_str
    )
);

named!(i32_digit<i32>,
    map_res!(
        map_res!(
            digit,
            str::from_utf8
        ),
    FromStr::from_str
    )
);

named!(pub parse_person<&[u8],Person>,
    chain!(
        name: map_res!(take_until_and_consume!(" <"), str::from_utf8) ~
        email: map_res!(take_until_and_consume!("> "), str::from_utf8) ~
        ts: u64_digit ~
        space ~
        sign: alt!(char!('+') | char!('-')) ~
        tz: i32_digit ~
        newline,
        || {
            let sgn = if sign == '-' {
                -1
            } else {
                1
            };
            let naive = NaiveDateTime::from_timestamp(ts as i64, 0);
            let offset = FixedOffset::east(sgn * tz/100 * 3600);
            let timestamp = DateTime::from_utc(naive, offset);
            Person {
                name,
                email,
                timestamp,
            }
        }
    )
);

named!(parse_commit_inner<&[u8], (Vec<u8>, Vec<Vec<u8>>, Person, Person, &str)>,
  chain!(
    tag!("tree ") ~
    tree: map!(take!(40), |hex_sha| {
        let mut sha = vec![0; 20];
        hex_decode(hex_sha, &mut sha).expect("sizes should be valid by parsing");
        sha
    }) ~
    newline ~
    parents: many0!(
        chain!(
            tag!("parent ") ~
            parent: map!(take!(40), |hex_sha| {
                let mut sha = vec![0; 20];
                hex_decode(hex_sha, &mut sha).expect("sizes should be valid by parsing");
                sha
            }) ~
            newline ,
            || { parent }
        )
    ) ~
    tag!("author ") ~
    author: parse_person ~
    tag!("committer ") ~
    committer: parse_person ~
    line_ending ~
    message: map_res!(rest, str::from_utf8),
    || (tree, parents, author, committer, message)
  )
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{GitObject, GitObjectType};
    use nom::IResult;
    use faster_hex::hex_decode;

    #[test]
    fn test_person_parsing() {
        let input = b"The Author <author@devs.com> 1353116070 +1100\n";

        if let IResult::Done(_, person) = parse_person(&input[..]) {
            assert_eq!(person.name, "The Author");
            assert_eq!(person.email, "author@devs.com");
        }
    }

    #[test]
    fn test_commit_parsing() {
        let input = b"tree abdf456789012345678901234567890123456789\n\
            parent abcdefaaa012345678901234567890123456789a\n\
            parent abcdefbbb012345678901234567890123456789a\n\
            author The Author <author@devs.com> 1353116070 +1100\n\
            committer The Committer <commiter@devs.com> 1353116070 +1100\n\
            \n\
            Bump version to 1.6";
        let input2 = b"tree 9f5829a852fcd8e3381e343b45cb1c9ff33abf56\nauthor Christian Briones <christian@whisper.sh> 1418004896 -0800\ncommitter Christian Briones <christian@whisper.sh> 1418004914 -0800\n\ninit\n";
        let object = GitObject::new(GitObjectType::Commit, (&input[..]).to_owned());
        if let Some(commit) = Commit::from_raw(&object) {
            assert_eq!(&commit.tree[..], decode_hex_sha(b"abdf456789012345678901234567890123456789"));
            let parents = vec![
                decode_hex_sha(b"abcdefaaa012345678901234567890123456789a"),
                decode_hex_sha(b"abcdefbbb012345678901234567890123456789a"),
            ];
            assert_eq!(commit.parents, parents);
            assert_eq!(commit.message, "Bump version to 1.6");
        } else {
            panic!("Failed to parse commit.");
        }

        let object2 = GitObject::new(GitObjectType::Commit, (&input2[..]).to_owned());
        assert!(Commit::from_raw(&object2).is_some())
    }

    fn decode_hex_sha(hex: &[u8]) -> [u8; 20] {
        let mut sha = [0u8; 20];
        hex_decode(hex, &mut sha).unwrap();
        sha
    }
}
