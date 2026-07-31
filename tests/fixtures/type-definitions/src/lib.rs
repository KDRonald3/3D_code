//! Fixture for emitted type nodes and field type_refs.

pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub struct Named {
    pub label: Label,
    pub at: Point,
}

pub enum Label {
    Short(Tag),
    Long,
}

pub struct Tag {
    pub code: u8,
}

pub type Alias = Named;

pub fn make() -> Named {
    Named {
        label: Label::Long,
        at: Point { x: 0, y: 0 },
    }
}
