use nom::{IResult, rest, newline, line_ending};

use std::str;

#[derive(Debug)]
pub struct Person {
    name: String,
    email: String,
    timestamp: String
}

#[derive(Debug)]
pub struct Commit {
    pub tree: String,
    parents: Vec<String>,
    author: Person,
    committer: Person,
    message: String
}

impl Commit {
    pub fn parse(content: &[u8]) -> Option<Self> {
        if let IResult::Done(_, raw_parts) = parse_commit_inner(content) {
            let (tree, parents, author, committer, message) = raw_parts;
            Some(Commit {
                tree: tree.to_owned(),
                parents: parents,
                author: author,
                committer: committer,
                message: message.to_owned()
            })
        } else {
            None
        }
    }
}

named!(parse_person<&[u8],Person>,
    chain!(
        name: map_res!(take_until_and_consume!(" <"), str::from_utf8) ~
        email: map_res!(take_until_and_consume!("> "), str::from_utf8) ~
        ts: map_res!(take_until_and_consume!("\n"), str::from_utf8),
        || {
            Person {
                name: name.to_owned(),
                email: email.to_owned(),
                timestamp: ts.to_owned()
            }
        }
    )
);

named!(parse_commit_inner<&[u8], (&str, Vec<String>, Person, Person, &str)>,
  chain!(
    tag!("tree ") ~
    tree: map_res!(take!(40), str::from_utf8) ~
    newline ~
    parents: many1!(
        chain!(
            tag!("parent ") ~
            parent: map_res!(take!(40), str::from_utf8) ~
            newline ,
            || { parent.to_owned() }
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
    let input = "tree tree456789012345678901234567890123456789\n\
        parent parentone012345678901234567890123456789a\n\
        parent parenttwo012345678901234567890123456789b\n\
        author The Author <author@devs.com> 1353116070 +1100\n\
        committer The Committer <commiter@devs.com> 1353116070 +1100\n\
        \n\
        Bump version to 1.6";

    if let Some(commit) = Commit::parse(input.as_bytes()) {
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
