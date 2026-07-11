//! Conditional tag rules: per-Poster eligibility constraints beyond flat
//! subscribe/forbid lists.
//!
//! Syntax (also the storage form): `[antecedent…]->[consequent…]`, where
//! each side is whitespace-separated *terms*. A term is a literal — `tag`
//! (must be present) or `-tag` (must be absent) — or an OR-group
//! `(literal literal …)` that holds when AT LEAST ONE of its literals
//! holds. If ALL antecedent terms hold for an entry's effective tags, ALL
//! consequent terms must hold too, otherwise the entry is skipped for this
//! consumer only.
//!
//! Examples: a straight channel forbidding ambiguous solo content:
//! `[solo]->[-male]`; requiring an orientation marker on every duo:
//! `[duo]->[(gay straight bisexual)]`.
//!
//! [`TagTerm`] is also the unit of Poster tag subscriptions — one grammar
//! everywhere tags filter.

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

impl TagLiteral {
    /// The natural-language reading: `solo` / `NO female` (parenthesized
    /// inside OR-groups by [`TagTerm::describe`]).
    fn describe(&self) -> String {
        match self {
            TagLiteral::Has(tag) => tag.to_string(),
            TagLiteral::Lacks(tag) => format!("NO {tag}"),
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

/// One conjunction unit: a single literal, or an OR-group `(a b -c)` that
/// is satisfied when at least one of its literals holds. A bare literal is
/// just a one-element group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagTerm(pub Vec<TagLiteral>);

impl TagTerm {
    pub fn passes(&self, tags: &HashSet<Tag>) -> bool {
        self.0.iter().any(|literal| literal.holds(tags))
    }

    /// Parse a whitespace-separated list of terms, honoring `(…)` groups.
    /// The empty string is an empty (always-true) list.
    pub fn parse_list(raw: &str) -> Result<Vec<TagTerm>, TagRuleParseError> {
        let mut terms = Vec::new();
        let mut rest = raw.trim_start();
        while !rest.is_empty() {
            if let Some(after_open) = rest.strip_prefix('(') {
                let close = after_open
                    .find(')')
                    .ok_or(TagRuleParseError::UnclosedGroup)?;
                let inner = &after_open[..close];
                if inner.contains('(') {
                    return Err(TagRuleParseError::NestedGroup);
                }
                let literals: Vec<TagLiteral> = inner
                    .split_whitespace()
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
                if literals.is_empty() {
                    return Err(TagRuleParseError::EmptyGroup);
                }
                terms.push(TagTerm(literals));
                rest = after_open[close + 1..].trim_start();
            } else {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                let word = &rest[..end];
                if word.contains('(') || word.contains(')') {
                    return Err(TagRuleParseError::StrayParen);
                }
                terms.push(TagTerm(vec![word.parse()?]));
                rest = rest[end..].trim_start();
            }
        }
        Ok(terms)
    }
}

impl TagTerm {
    /// The natural-language reading of one term: a bare literal reads as
    /// itself (`solo`, `NO male`); an OR-group reads as
    /// `((NO female) OR intersex)` — negations parenthesized so the OR
    /// binds visibly.
    pub fn describe(&self) -> String {
        match self.0.as_slice() {
            [single] => single.describe(),
            many => {
                let joined = many
                    .iter()
                    .map(|literal| match literal {
                        TagLiteral::Has(_) => literal.describe(),
                        TagLiteral::Lacks(_) => format!("({})", literal.describe()),
                    })
                    .collect::<Vec<_>>()
                    .join(" OR ");
                format!("({joined})")
            }
        }
    }

    /// A whole conjunction of terms: `solo AND avian`.
    pub fn describe_list(terms: &[TagTerm]) -> String {
        terms
            .iter()
            .map(TagTerm::describe)
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

/// A bare tag is the canonical singleton term.
impl From<Tag> for TagTerm {
    fn from(tag: Tag) -> Self {
        TagTerm(vec![TagLiteral::Has(tag)])
    }
}

impl std::fmt::Display for TagTerm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.as_slice() {
            [single] => write!(f, "{single}"),
            many => {
                let joined = many
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" ");
                write!(f, "({joined})")
            }
        }
    }
}

/// `[if_all…]->[then_all…]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRule {
    pub if_all: Vec<TagTerm>,
    pub then_all: Vec<TagTerm>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TagRuleParseError {
    #[error("rule must look like [tag tag]->[tag -tag]")]
    Malformed,
    #[error("empty tag literal")]
    EmptyLiteral,
    #[error("both sides of a rule need at least one literal")]
    EmptySide,
    #[error("unclosed ( group")]
    UnclosedGroup,
    #[error("empty () group")]
    EmptyGroup,
    #[error("nested ( groups are not supported")]
    NestedGroup,
    #[error("( and ) must wrap whole groups, not sit inside a tag")]
    StrayParen,
}

impl TagRule {
    /// Whether the entry's tags satisfy this rule (vacuously true when the
    /// antecedent doesn't fully match).
    pub fn passes(&self, tags: &HashSet<Tag>) -> bool {
        if !self.if_all.iter().all(|term| term.passes(tags)) {
            return true;
        }
        self.then_all.iter().all(|term| term.passes(tags))
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

    /// The natural-language reading of the whole rule:
    /// `[solo avian]->[(-female intersex) bird]` reads as
    /// `solo AND avian REQUIRE ((NO female) OR intersex) AND bird`.
    pub fn describe(&self) -> String {
        format!(
            "{} REQUIRE {}",
            TagTerm::describe_list(&self.if_all),
            TagTerm::describe_list(&self.then_all)
        )
    }
}

impl std::fmt::Display for TagRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let side = |terms: &[TagTerm]| {
            terms
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
        let parse_side = |side: &str| -> Result<Vec<TagTerm>, TagRuleParseError> {
            let terms = TagTerm::parse_list(side)?;
            if terms.is_empty() {
                return Err(TagRuleParseError::EmptySide);
            }
            Ok(terms)
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
    fn or_groups_hold_on_any_hit() {
        let terms = TagTerm::parse_list("male (gay bisexual) -cub").unwrap();
        assert_eq!(terms.len(), 3);
        let ok = |names: &[&str]| terms.iter().all(|t| t.passes(&tags(names)));
        assert!(ok(&["male", "gay"]));
        assert!(ok(&["male", "bisexual"]));
        assert!(!ok(&["male", "straight"])); // no orientation hit
        assert!(!ok(&["male", "gay", "cub"])); // -cub violated

        // Display/parse roundtrip keeps the group form.
        let printed = terms
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(printed, "male (gay bisexual) -cub");
        assert_eq!(TagTerm::parse_list(&printed).unwrap(), terms);
    }

    #[test]
    fn or_groups_inside_rules() {
        // Every duo must carry an orientation marker.
        let rule: TagRule = "[duo]->[(gay straight bisexual)]".parse().unwrap();
        assert!(rule.passes(&tags(&["duo", "gay"])));
        assert!(rule.passes(&tags(&["duo", "straight"])));
        assert!(!rule.passes(&tags(&["duo", "male"])));
        assert!(rule.passes(&tags(&["solo", "male"]))); // vacuous
        assert_eq!(rule.to_string(), "[duo]->[(gay straight bisexual)]");
    }

    #[test]
    fn group_parse_errors() {
        assert_eq!(
            TagTerm::parse_list("(gay bisexual"),
            Err(TagRuleParseError::UnclosedGroup)
        );
        assert_eq!(
            TagTerm::parse_list("()"),
            Err(TagRuleParseError::EmptyGroup)
        );
        assert_eq!(
            TagTerm::parse_list("ga)y"),
            Err(TagRuleParseError::StrayParen)
        );
        assert_eq!(
            TagTerm::parse_list("((a b))"),
            Err(TagRuleParseError::NestedGroup)
        );
    }

    #[test]
    fn describe_reads_as_mechanical_natural_language() {
        let rule: TagRule = "[solo avian]->[(-female intersex) bird]".parse().unwrap();
        assert_eq!(
            rule.describe(),
            "solo AND avian REQUIRE ((NO female) OR intersex) AND bird"
        );
        let simple: TagRule = "[solo]->[-male]".parse().unwrap();
        assert_eq!(simple.describe(), "solo REQUIRE NO male");
        // Terms describe on their own too (poster subscriptions).
        let terms = TagTerm::parse_list("wolf (male female)").unwrap();
        assert_eq!(
            TagTerm::describe_list(&terms),
            "wolf AND (male OR female)"
        );
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
