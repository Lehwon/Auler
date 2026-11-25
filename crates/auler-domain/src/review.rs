//! Row-level review data shared by the import paths.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::*;
use crate::resolve::{Issue, Severity};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    New,
    Update,
    Unchanged,
    NeedsReview,
    Error,
}

impl Disposition {
    pub fn code(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Update => "update",
            Self::Unchanged => "unchanged",
            Self::NeedsReview => "needs_review",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ReviewValue {
    Setup(TermSetup),
    Equipment(EquipmentKind),
    Room(Room),
    Instructor(Instructor),
    Group(StudentGroup),
    Course(Course),
    Section(Section),
    Constraint(Constraint),
    Placement(Placement),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewItem {
    /// Stable path in the submitted document, such as `sections[0]`.
    pub path: String,
    pub source: Option<SourceRef>,
    pub input: Value,
    pub before: Option<ReviewValue>,
    pub after: Option<ReviewValue>,
    pub issues: Vec<Issue>,
    /// Other rows that must be included or resolved for this proposal.
    pub prerequisites: Vec<String>,
}

impl ReviewItem {
    pub fn disposition(&self) -> Disposition {
        match self.issues.iter().map(Issue::severity).max() {
            Some(Severity::Error) => Disposition::Error,
            Some(Severity::NeedsReview) => Disposition::NeedsReview,
            None if self.after.is_none() => Disposition::Error,
            None if self.before == self.after => Disposition::Unchanged,
            None if self.before.is_some() => Disposition::Update,
            None => Disposition::New,
        }
    }
}

/// Current draft and workspace-wide data. A new term has no setup yet.
#[derive(Default)]
pub struct ReviewBase {
    pub setup: Option<TermSetup>,
    pub equipment: Vec<EquipmentKind>,
    pub rooms: Vec<Room>,
    pub instructors: Vec<Instructor>,
    pub groups: Vec<StudentGroup>,
    pub courses: Vec<Course>,
    pub sections: Vec<Section>,
    pub constraints: Vec<Constraint>,
    pub placements: Vec<Placement>,
}

#[derive(Debug)]
pub struct ReviewPreview {
    pub term: Option<TermKey>,
    pub issues: Vec<Issue>,
    /// Dependency order, so a selected record can refer to earlier selected records.
    pub items: Vec<ReviewItem>,
}
