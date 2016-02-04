use packfile::{PackObject, PackObjectType};
use store::commit::Commit;
use store::tree::Tree;

pub enum ObjectType {
    Tree,
    Commit,
    Tag,
    Blob
}

pub struct Object {
    pub obj_type: ObjectType,
    pub content: Vec<u8>,
}

impl Object {
    pub fn from_raw(raw: PackObject) -> Option<Self> {
        let obj_type = match raw.obj_type {
            PackObjectType::Commit => Some(ObjectType::Commit),
            PackObjectType::Tag => Some(ObjectType::Tag),
            PackObjectType::Tree => Some(ObjectType::Tree),
            PackObjectType::Blob => Some(ObjectType::Blob),
            _ => None
        };
        let content = raw.content;
        obj_type.map(|t| {
            Object {
                obj_type: t,
                content: content
            }
        })
    }
}

impl Object {
    pub fn as_tree(&self) -> Option<Tree> {
        if let ObjectType::Tree = self.obj_type {
            Tree::parse(&self.content)
        } else {
            None
        }
    }

    pub fn as_commit(&self) -> Option<Commit> {
        if let ObjectType::Commit = self.obj_type {
            Commit::parse(&self.content)
        } else {
            None
        }
    }
}
