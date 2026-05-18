/// A label that describes a Post.
///
/// Tags are value objects: two `Tag`s with the same name compare equal.
///
/// Per `design/domain.md`:
/// - Tags describe e621-sourced posts; non-e621 posts have zero tags.
/// - A Poster's `subscribed_tags` apply as required filters when querying e621.
/// - A Poster's `forbidden_tags` exclude any matching post (one is enough to disqualify).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag(String);

impl AsRef<str> for Tag {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Tag {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<String> for Tag {
    fn from(value: String) -> Self {
        Self(value)
    }
}
