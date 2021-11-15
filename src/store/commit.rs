use std::fmt::{
    self,
    Display,
    Formatter,
};
use std::str;

use chrono::naive::NaiveDateTime;
use chrono::offset::FixedOffset;
use chrono::DateTime;
use nom::bytes::complete as bytes;
use nom::bytes::complete::tag;
use nom::bytes::complete::{
    take,
    take_until,
};
use nom::character::complete as character;
use nom::combinator::map;
use nom::combinator::map_res;
use nom::sequence;
use nom::IResult;

use crate::store::PackedObject;
use crate::store::Sha;

pub struct Person<'a> {
    name: &'a str,
    email: &'a str,
    timestamp: DateTime<FixedOffset>,
}

pub struct Commit<'a> {
    pub tree: Sha,
    pub parents: Vec<Sha>,
    author: Person<'a>,
    #[allow(dead_code)]
    committer: Person<'a>,
    message: &'a str,
    sha: Sha,
}

impl<'a> Commit<'a> {
    pub fn from_raw(raw: &'a PackedObject) -> Option<Self> {
        let sha = raw.sha();
        match parse_commit::<(&[u8], nom::error::ErrorKind)>(&raw.content, sha) {
            IResult::Ok(([], commit)) => Some(commit),
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
        writeln!(f, "commit {}", self.sha.hex())?;
        write!(f, "{}", self.author)?;
        for line in self.message.split('\n') {
            write!(f, "\n    {}", line)?;
        }
        // It's not clear if this is expected, but some commit
        // messages can lack a final newline.
        if !self.message.ends_with('\n') {
            writeln!(f)?;
        }
        Ok(())
    }
}

fn parse_person<'a, E>(input: &'a [u8]) -> IResult<&'a [u8], Person, E>
where
    E: nom::error::ParseError<&'a [u8]>,
    E: nom::error::FromExternalError<&'a [u8], str::Utf8Error>,
{
    let parts = sequence::tuple((
        map_res(
            sequence::terminated(take_until(" <"), take(2usize)),
            str::from_utf8,
        ),
        map_res(
            sequence::terminated(take_until("> "), take(2usize)),
            str::from_utf8,
        ),
        sequence::terminated(character::i64, character::char(' ')),
        sequence::terminated(character::i32, character::newline),
    ));
    // FIXME: Why is this str and not bytes
    map(parts, |(name, email, ts, tz)| {
        let naive = NaiveDateTime::from_timestamp(ts, 0);
        let offset = FixedOffset::east(tz / 100 * 3600);
        let timestamp = DateTime::from_utc(naive, offset);
        Person {
            name,
            email,
            timestamp,
        }
    })(input)
}

fn gpgsig<'a, E>(input: &'a [u8]) -> IResult<&'a [u8], (), E>
where
    E: nom::error::ParseError<&'a [u8]>,
{
    let parts = sequence::tuple((
        tag("gpgsig"),
        sequence::terminated(
            bytes::take_till1(nom::character::is_newline),
            character::newline,
        ),
        nom::multi::many0(sequence::preceded(
            character::char(' '),
            sequence::terminated(
                bytes::take_till(nom::character::is_newline),
                character::newline,
            ),
        )),
    ));
    map(parts, |_| ())(input)
}

fn parse_commit<'a, E>(input: &'a [u8], sha: Sha) -> IResult<&'a [u8], Commit<'a>, E>
where
    E: nom::error::ParseError<&'a [u8]>,
    E: nom::error::FromExternalError<&'a [u8], str::Utf8Error>,
    E: nom::error::FromExternalError<&'a [u8], super::DecodeShaError>,
{
    let parts = sequence::tuple((
        sequence::preceded(
            tag("tree "),
            map_res(
                bytes::take(40usize),
                // FIXME: This and parent below can use unchecked by
                // making the parser hex-aware
                Sha::from_hex,
            ),
        ),
        character::newline,
        nom::multi::many0(sequence::terminated(
            sequence::preceded(tag("parent "), map_res(bytes::take(40usize), Sha::from_hex)),
            character::newline,
        )),
        sequence::preceded(tag("author "), parse_person),
        sequence::preceded(tag("committer "), parse_person),
        nom::combinator::opt(gpgsig),
        character::newline,
        map_res(nom::combinator::rest, str::from_utf8),
    ));
    map(
        parts,
        |(tree, _, parents, author, committer, _, _, message)| Commit {
            tree,
            parents,
            author,
            committer,
            message,
            sha: sha.clone(),
        },
    )(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        ObjectType,
        PackedObject,
    };

    #[test]
    fn test_parse_person() {
        let input = b"The Author <author@devs.com> 1353116070 +1100\n";

        if let Ok((_, person)) = parse_person::<nom::error::Error<_>>(&input[..]) {
            assert_eq!(person.name, "The Author");
            assert_eq!(person.email, "author@devs.com");
        }
    }

    #[test]
    fn test_parse_commit() {
        let input = b"tree abdf456789012345678901234567890123456789\n\
            parent abcdefaaa012345678901234567890123456789a\n\
            parent abcdefbbb012345678901234567890123456789a\n\
            author The Author <author@devs.com> 1353116070 +1100\n\
            committer The Committer <commiter@devs.com> 1353116070 +1100\n\
            \n\
            Bump version to 1.6";
        let input2 = b"tree 9f5829a852fcd8e3381e343b45cb1c9ff33abf56\nauthor Christian Briones <cwbriones@gmail.com> 1418004896 -0800\ncommitter Christian Briones <cwbriones@gmail.com> 1418004914 -0800\n\ninit\n";
        let sha = Sha::from_bytes(&[0u8; 20][..]).unwrap();
        let (_, commit) = parse_commit::<nom::error::VerboseError<&[u8]>>(&input[..], sha)
            .expect("failed to parse commit");
        assert_eq!(
            commit.tree.hex(),
            "abdf456789012345678901234567890123456789"
        );
        assert_eq!(
            commit.parents[0].hex(),
            "abcdefaaa012345678901234567890123456789a"
        );
        assert_eq!(
            commit.parents[1].hex(),
            "abcdefbbb012345678901234567890123456789a"
        );
        assert_eq!(commit.message, "Bump version to 1.6");

        let object2 = PackedObject::new(ObjectType::Commit, (&input2[..]).to_owned());
        assert!(Commit::from_raw(&object2).is_some())
    }

    #[test]
    fn test_commit_with_gpg_field() {
        let input = "tree 639020696c82665786f02e6081336171c4afafad\n\
                      parent 91e34e77c97bd44eab14e6fe6b636b2588a269cc\n\
                      author Jon Gjengset <jon@thesquareplanet.com> 1625115559 -0700\n\
                      committer Jon Gjengset <jon@thesquareplanet.com> 1625115559 -0700\n\
                      gpgsig -----BEGIN PGP SIGNATURE-----\n \n iHUEABYKAB0WIQRIAlMG/9GjsFwKNi2GO0ihHCONWgUCYN1LpwAKCRCGO0ihHCON\n Woa/AP9c3+/Yw8Yr6VfS8fsU4s/5Vq+uFmnEhAC6Y6iSdjVO+wEA+0cAR061hopo\n 5wD8cNB/3HInkW9RT/C+A31I6mTgKQU=\n =h5wm\n -----END PGP SIGNATURE-----\n\nMissed a clippy lint in rayon behind feature\n";

        let sha = Sha::from_bytes(&[0u8; 20][..]).unwrap();
        match parse_commit::<nom::error::VerboseError<_>>(input.as_bytes(), sha) {
            Err(nom::Err::Failure(err)) => {
                for (ctx, e) in err.errors {
                    panic!(
                        "Failed to parse commit: {:?}: {:?}",
                        str::from_utf8(ctx).unwrap(),
                        e
                    );
                }
            }
            Err(nom::Err::Incomplete(_)) => {
                panic!("Failed to parse commit: unexpected EOF");
            }
            Err(nom::Err::Error(err)) => {
                for (ctx, e) in err.errors {
                    panic!(
                        "Failed to parse commit: {}: {:?}",
                        str::from_utf8(ctx).unwrap(),
                        e
                    );
                }
            }
            Ok((_, commit)) => {
                assert_eq!(
                    commit.message,
                    "Missed a clippy lint in rayon behind feature\n"
                );
            }
        }
    }
}
