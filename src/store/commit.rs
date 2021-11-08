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
    opt,
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

use crate::store::PackedObject;
use crate::store::Sha;

pub struct Person<'a> {
    name: &'a str,
    email: &'a str,
    timestamp: DateTime<FixedOffset>
}

pub struct Commit<'a> {
    pub tree: Sha,
    pub parents: Vec<Sha>,
    author: Person<'a>,
    #[allow(dead_code)]
    committer: Person<'a>,
    message: &'a str,
    raw: &'a PackedObject
}

impl<'a> Commit<'a> {
    pub fn from_raw(raw: &'a PackedObject) -> Option<Self> {
        match parse_commit_inner(&raw.content) {
            IResult::Done(_, raw_parts) => {
                let (tree, parents, author, committer, message) = raw_parts;
                Some(Commit {
                    tree,
                    parents,
                    author,
                    committer,
                    message,
                    raw,
                })
            },
            _ => None,
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
        writeln!(f, "commit {}", self.raw.sha().hex())?;
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

named!(gpgsig<&[u8], ()>,
  chain!(
    tag!("gpgsig")
    ~
    _begin: take_until_and_consume!("\n")
    ~
    _content: many0!(
      chain!(
          space
          ~
          take_until_and_consume!("\n"),
          || ()
      )
    ),
    || ()
  )
);

named!(parse_commit_inner<&[u8], (Sha, Vec<Sha>, Person, Person, &str)>,
  chain!(
    tag!("tree ") ~
    tree: map!(take!(40), |hex_sha| {
        // FIXME: This and parent below can use unchecked by
        // making the parser hex-aware
        Sha::from_hex(hex_sha).expect("parsed hex was invalid")
    }) ~
    newline ~
    parents: many0!(
        chain!(
            tag!("parent ") ~
            parent: map!(take!(40), |hex_sha| {
                Sha::from_hex(hex_sha).expect("parsed hex was invalid")
            }) ~
            newline ,
            || { parent }
        )
    ) ~
    tag!("author ") ~
    author: parse_person ~
    tag!("committer ") ~
    committer: parse_person ~
    _gpg: opt!(gpgsig) ~
    line_ending ~
    message: map_res!(rest, str::from_utf8),
    || (tree, parents, author, committer, message)
  )
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{PackedObject, ObjectType};
    use nom::IResult;
    

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
        let object = PackedObject::new(ObjectType::Commit, (&input[..]).to_owned());
        if let Some(commit) = Commit::from_raw(&object) {
            assert_eq!(commit.tree, Sha::from_hex(b"abdf456789012345678901234567890123456789").unwrap());
            let parents = vec![
                Sha::from_hex(b"abcdefaaa012345678901234567890123456789a").unwrap(),
                Sha::from_hex(b"abcdefbbb012345678901234567890123456789a").unwrap(),
            ];
            assert_eq!(commit.parents, parents);
            assert_eq!(commit.message, "Bump version to 1.6");
        } else {
            panic!("Failed to parse commit.");
        }

        let object2 = PackedObject::new(ObjectType::Commit, (&input2[..]).to_owned());
        assert!(Commit::from_raw(&object2).is_some())
    }

    #[test]
    fn test_gpg_field() {
        let input = b"tree 639020696c82665786f02e6081336171c4afafad\n\
                      parent 91e34e77c97bd44eab14e6fe6b636b2588a269cc\n\
                      author Jon Gjengset <jon@thesquareplanet.com> 1625115559 -0700\n\
                      committer Jon Gjengset <jon@thesquareplanet.com> 1625115559 -0700\n\
                      gpgsig -----BEGIN PGP SIGNATURE-----\n \n iHUEABYKAB0WIQRIAlMG/9GjsFwKNi2GO0ihHCONWgUCYN1LpwAKCRCGO0ihHCON\n Woa/AP9c3+/Yw8Yr6VfS8fsU4s/5Vq+uFmnEhAC6Y6iSdjVO+wEA+0cAR061hopo\n 5wD8cNB/3HInkW9RT/C+A31I6mTgKQU=\n =h5wm\n -----END PGP SIGNATURE-----\n\nMissed a clippy lint in rayon behind feature\n";

        let object = PackedObject::new(ObjectType::Commit, (&input[..]).to_owned());
        let _commit = Commit::from_raw(&object).expect("failed to parse commit");
    }
}
