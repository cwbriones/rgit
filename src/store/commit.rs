use nom::{IResult, rest, newline, line_ending, digit, space};

use std::str::{self, FromStr};
use store::GitObject;

use std::fmt::{self, Display, Formatter};
use chrono::naive::datetime::NaiveDateTime;
use chrono::datetime::DateTime;
use chrono::offset::fixed::FixedOffset;

pub struct Person<'a> {
    name: &'a str,
    email: &'a str,
    timestamp: DateTime<FixedOffset>
}

pub struct Commit<'a> {
    pub tree: &'a str,
    pub parents: Vec<&'a str>,
    author: Person<'a>,
    _committer: Person<'a>,
    message: &'a str,
    raw: &'a GitObject
}

impl<'a> Commit<'a> {
    pub fn from_raw(obj: &'a GitObject) -> Option<Self> {
        if let IResult::Done(_, raw_parts) = parse_commit_inner(&obj.content) {
            let (tree, parents, author, committer, message) = raw_parts;
            Some(Commit {
                tree: tree,
                parents: parents,
                author: author,
                _committer: committer,
                message: message,
                raw: obj
            })
        } else {
            None
        }
    }
}

impl<'a> Display for Person<'a> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        try!(writeln!(f, "Author: {} <{}>", self.name, self.email));
        let formatted = self.timestamp.format("%a %b %-e %T %Y %z");
        try!(writeln!(f, "Date:   {}", formatted));
        Ok(())
    }
}

impl<'a> Display for Commit<'a> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        try!(writeln!(f, "commit {}", self.raw.sha()));
        try!(write!(f, "{}", self.author));
        for line in self.message.split('\n') {
            try!(write!(f, "\n    {}", line));
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
            let datetime = DateTime::from_utc(naive, offset);
            Person {
                name: name,
                email: email,
                timestamp: datetime
            }
        }
    )
);

named!(parse_commit_inner<&[u8], (&str, Vec<&str>, Person, Person, &str)>,
  chain!(
    tag!("tree ") ~
    tree: map_res!(take!(40), str::from_utf8) ~
    newline ~
    parents: many0!(
        chain!(
            tag!("parent ") ~
            parent: map_res!(take!(40), str::from_utf8) ~
            newline ,
            || { parent }
        )
    ) ~
    tag!("author ") ~
    author: parse_person ~
    tag!("committer ") ~
    committer: parse_person ~
    line_ending ~
    message: map_res!(rest, str::from_utf8) ,
    || {
        return (tree, parents, author, committer, message)
    }
  )
);

#[cfg(test)]
mod tests {
    use super::*;
    use store::{GitObject, GitObjectType};
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
        let input = b"tree asdf456789012345678901234567890123456789\n\
            parent parentone012345678901234567890123456789a\n\
            parent parenttwo012345678901234567890123456789b\n\
            author The Author <author@devs.com> 1353116070 +1100\n\
            committer The Committer <commiter@devs.com> 1353116070 +1100\n\
            \n\
            Bump version to 1.6";
        let input2 = b"tree 9f5829a852fcd8e3381e343b45cb1c9ff33abf56\nauthor Christian Briones <christian@whisper.sh> 1418004896 -0800\ncommitter Christian Briones <christian@whisper.sh> 1418004914 -0800\n\ninit\n";
        let object = GitObject::new(GitObjectType::Commit, (&input[..]).to_owned());
        if let Some(commit) = Commit::from_raw(&object) {
            assert_eq!(commit.tree, "asdf456789012345678901234567890123456789");
            let parents = vec![
                "parentone012345678901234567890123456789a",
                "parenttwo012345678901234567890123456789b"
            ];
            assert_eq!(commit.parents, parents);
            assert_eq!(commit.message, "Bump version to 1.6");
        } else {
            panic!("Failed to parse commit.");
        }

        let object2 = GitObject::new(GitObjectType::Commit, (&input2[..]).to_owned());
        assert!(Commit::from_raw(&object2).is_some())
    }
}
