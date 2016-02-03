use nom::{IResult, rest, newline, line_ending};

use std::str;

use packfile;

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
    sha: String,
    author: Person,
    committer: Person,
    message: String
}

impl Commit {
    pub fn from_packfile_object(raw: &packfile::Object) -> Option<Self> {
        if let IResult::Done(_, raw_parts) = parse_commit_inner(&raw.content[..]) {
            let (tree, parents, author, committer, message) = raw_parts;
            Some(Commit {
                tree: tree.to_owned(),
                parents: parents,
                author: author,
                committer: committer,
                sha: raw.sha(),
                message: message.to_owned()
            })
        } else {
            println!("failed to parse commit... :(");
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

#[allow(unused)]
fn parse_commit(input: &[u8], sha: String) -> IResult<&[u8], Commit> {
    match parse_commit_inner(input) {
        IResult::Done(buf, raw_parts) => {
            let (tree, parents, author, committer, message) = raw_parts;
            IResult::Done(buf, Commit {
                tree: tree.to_owned(),
                parents: parents,
                author: author,
                committer: committer,
                message: message.to_owned(),
                sha: sha
            })
        },
        IResult::Incomplete(needed) => IResult::Incomplete(needed),
        IResult::Error(e) => IResult::Error(e)
    }
}

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
    let sha = "sha".to_owned();

    if let IResult::Done(_, commit) = parse_commit(input.as_bytes(), sha) {
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
