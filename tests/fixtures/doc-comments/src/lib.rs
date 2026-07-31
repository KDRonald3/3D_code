//! File-level module docs for the doc-comments fixture.
//! Second inner line.

/// Single-line outer docs on alpha.
pub fn alpha() {}

/// First outer line on beta.
/// Second outer line on beta.
pub fn beta() {}

/// Docs separated from the item by an attribute.
#[inline]
pub fn gamma() {}

/**Block outer docs on delta.*/
pub fn delta() {}

// Ordinary line comment — must not be collected as documentation.
pub fn epsilon() {}

#[doc = "Attribute-form docs on zeta."]
pub fn zeta() {}
