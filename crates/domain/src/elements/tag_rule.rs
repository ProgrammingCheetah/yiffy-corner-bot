//! Conditional tag rules: per-Poster eligibility constraints beyond flat
//! subscribe/forbid lists.
//!
//! Syntax (also the storage form): `[antecedent…]->[consequent…]`, where
//! each side is whitespace-separated literals — `tag` (must be present) or
//! `-tag` (must be absent). If ALL antecedent literals hold for an entry's
//! effective tags, ALL consequent literals must hold too, otherwise the
//! entry is skipped for this consumer only.
//!
//! Example: a straight channel forbidding ambiguous solo content:
//! `[solo]->[-male]` — solo posts are eligible only when `male` is absent.

use std::collections::HashSet;

use crate::elements::tag::Tag;

/// One side's element: a tag that must be present, or absent (`-tag`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagLiteral {
    Has(Tag),
    Lacks(Tag),
}

impl TagLiteral {
    fn holds(&self, tags: &HashSet<Tag>) -> bool {
        match self {
            TagLiteral::Has(tag) => tags.contains(tag),
            TagLiteral::Lacks(tag) => !tags.contains(tag),
        }
    }
}

impl std::fmt::Display for TagLiteral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TagLiteral::Has(tag) => write!(f, "{tag}"),
            TagLiteral::Lacks(tag) => write!(f, "-{tag}"),
        }
    }
}

impl std::str::FromStr for TagLiteral {
    type Err = TagRuleParseError;
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.strip_prefix('-') {
            Some("") | None if raw.is_empty() => Err(TagRuleParseError::EmptyLiteral),
            Some("") => Err(TagRuleParseError::EmptyLiteral),
            Some(name) => Ok(TagLiteral::Lacks(Tag::from(name))),
            None => Ok(TagLiteral::Has(Tag::from(raw))),
        }
    }
}

/// `[if_all…]->[then_all…]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRule {
    pub if_all: Vec<TagLiteral>,
    pub then_all: Vec<TagLiteral>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TagRuleParseError {
    #[error("rule must look like [tag tag]->[tag -tag]")]
    Malformed,
    #[error("empty tag literal")]
    EmptyLiteral,
    #[error("both sides of a rule need at least one literal")]
    EmptySide,
}

impl TagRule {
    /// Whether the entry's tags satisfy this rule (vacuously true when the
    /// antecedent doesn't fully match).
    pub fn passes(&self, tags: &HashSet<Tag>) -> bool {
        if !self.if_all.iter().all(|literal| literal.holds(tags)) {
            return true;
        }
        self.then_all.iter().all(|literal| literal.holds(tags))
    }

    /// Parse every `[…]->[…]` rule in a free-form string (the storage and
    /// command-argument format). Errors on any malformed fragment.
    pub fn parse_all(raw: &str) -> Result<Vec<TagRule>, TagRuleParseError> {
        let mut rules = Vec::new();
        let mut rest = raw.trim();
        while !rest.is_empty() {
            let Some(after_open) = rest.strip_prefix('[') else {
                return Err(TagRuleParseError::Malformed);
            };
            let Some(close) = after_open.find(']') else {
                return Err(TagRuleParseError::Malformed);
            };
            let left = &after_open[..close];
            let Some(after_arrow) = after_open[close + 1..].strip_prefix("->[") else {
                return Err(TagRuleParseError::Malformed);
            };
            let Some(close2) = after_arrow.find(']') else {
                return Err(TagRuleParseError::Malformed);
            };
            let right = &after_arrow[..close2];
            rules.push(format!("[{left}]->[{right}]").parse()?);
            rest = after_arrow[close2 + 1..].trim_start();
        }
        Ok(rules)
    }
}

impl std::fmt::Display for TagRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let side = |literals: &[TagLiteral]| {
            literals
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        };
        write!(f, "[{}]->[{}]", side(&self.if_all), side(&self.then_all))
    }
}

impl std::str::FromStr for TagRule {
    type Err = TagRuleParseError;
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let raw = raw.trim();
        let inner = raw.strip_prefix('[').ok_or(TagRuleParseError::Malformed)?;
        let (left, right_part) = inner
            .split_once("]->[")
            .ok_or(TagRuleParseError::Malformed)?;
        let right = right_part
            .strip_suffix(']')
            .ok_or(TagRuleParseError::Malformed)?;
        let parse_side = |side: &str| -> Result<Vec<TagLiteral>, TagRuleParseError> {
            let literals: Vec<TagLiteral> = side
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()?;
            if literals.is_empty() {
                return Err(TagRuleParseError::EmptySide);
            }
            Ok(literals)
        };
        Ok(TagRule {
            if_all: parse_side(left)?,
            then_all: parse_side(right)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(names: &[&str]) -> HashSet<Tag> {
        names.iter().map(|n| Tag::from(*n)).collect()
    }

    #[test]
    fn display_fromstr_roundtrip() {
        let rule: TagRule = "[solo]->[-male]".parse().unwrap();
        assert_eq!(rule.to_string(), "[solo]->[-male]");
        let rule: TagRule = "[solo female]->[safe -male]".parse().unwrap();
        assert_eq!(rule.to_string(), "[solo female]->[safe -male]");
    }

    #[test]
    fn straight_channel_example() {
        // "solo is only allowed without male"
        let rule: TagRule = "[solo]->[-male]".parse().unwrap();
        assert!(rule.passes(&tags(&["solo", "female"])));
        assert!(!rule.passes(&tags(&["solo", "male"])));
        // Not solo → rule is vacuous.
        assert!(rule.passes(&tags(&["male", "duo"])));

        // The strict variant: solo requires female.
        let rule: TagRule = "[solo]->[female]".parse().unwrap();
        assert!(rule.passes(&tags(&["solo", "female"])));
        assert!(!rule.passes(&tags(&["solo", "gynomorph"])));
    }

    #[test]
    fn negated_antecedents_work() {
        // "anything without female must carry gay"
        let rule: TagRule = "[-female]->[gay]".parse().unwrap();
        assert!(rule.passes(&tags(&["male", "gay"])));
        assert!(!rule.passes(&tags(&["male", "solo"])));
        assert!(rule.passes(&tags(&["female", "solo"])));
    }

    #[test]
    fn parse_all_handles_multiple_rules_and_garbage() {
        let rules = TagRule::parse_all("[solo]->[-male] [duo]->[female]").unwrap();
        assert_eq!(rules.len(), 2);
        assert!(TagRule::parse_all("").unwrap().is_empty());
        assert!(TagRule::parse_all("solo->female").is_err());
        assert!(TagRule::parse_all("[]->[male]").is_err());
        assert!(TagRule::parse_all("[solo]->[-]").is_err());
    }
}
