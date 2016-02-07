use nom::{IResult, rest, newline, line_ending};

use std::str;
use store::GitObject;

use std::fmt::{self, Display, Formatter};

pub struct Person<'a> {
    name: &'a str,
    email: &'a str,
    timestamp: &'a str
}

pub struct Commit<'a> {
    pub tree: &'a str,
    parents: Vec<&'a str>,
    author: Person<'a>,
    committer: Person<'a>,
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
                committer: committer,
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
        try!(writeln!(f, "Date:   {}", self.timestamp));
        Ok(())
    }
}

impl<'a> Display for Commit<'a> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        try!(writeln!(f, "commit {}", self.raw.sha()));
        try!(write!(f, "{}", self.author));
        for line in self.message.split("\n") {
            try!(write!(f, "\n    {}", line));
        }
        Ok(())
    }
}

named!(pub parse_person<&[u8],Person>,
    chain!(
        name: map_res!(take_until_and_consume!(" <"), str::from_utf8) ~
        email: map_res!(take_until_and_consume!("> "), str::from_utf8) ~
        ts: map_res!(take_until_and_consume!("\n"), str::from_utf8),
        || {
            Person {
                name: name,
                email: email,
                timestamp: ts,
            }
        }
    )
);

named!(parse_commit_inner<&[u8], (&str, Vec<&str>, Person, Person, &str)>,
  chain!(
    tag!("tree ") ~
    tree: map_res!(take!(40), str::from_utf8) ~
    newline ~
    parents: many1!(
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
        let input = "Some Person <person@people.com> 12345 +6789\n";

        if let IResult::Done(_, person) = parse_person(input.as_bytes()) {
            assert_eq!(person.name, "Some Person");
            assert_eq!(person.email, "person@people.com");
            assert_eq!(person.timestamp, "12345 +6789");
        } else {
            panic!("Failed to parse person.");
        }
    }

    #[test]
    fn test_commit_parsing() {
        let input = b"tree tree456789012345678901234567890123456789\n\
            parent parentone012345678901234567890123456789a\n\
            parent parenttwo012345678901234567890123456789b\n\
            author The Author <author@devs.com> 1353116070 +1100\n\
            committer The Committer <commiter@devs.com> 1353116070 +1100\n\
            \n\
            Bump version to 1.6";
        let object = GitObject {
            obj_type: GitObjectType::Commit,
            content: (&input[..]).to_owned(),
        };
        if let Some(commit) = Commit::from_raw(&object) {
            assert_eq!(commit.tree, "tree456789012345678901234567890123456789");
            let parents = vec![
                "parentone012345678901234567890123456789a",
                "parenttwo012345678901234567890123456789b"
            ];
            assert_eq!(commit.parents, parents);
            assert_eq!(commit.message, "Bump version to 1.6");
        } else {
            panic!("Failed to parse commit.");
        }
    }
}
