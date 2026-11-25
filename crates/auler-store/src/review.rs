//! Persistent import review. Only an explicit approval writes workspace data.

use std::collections::HashSet;

use auler_domain::input::InputDocument;
use auler_domain::model::{ConstraintStatus, TermKey};
use auler_domain::resolve::{Issue, IssueKind, inspect};
use auler_domain::review::ReviewBase;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::read;
use crate::writer::{Writer, now_ms, record_ops, validate_setup};
use crate::{Applied, ChangeMeta, Op, Origin, Record, Result, StoreError, Workspace};

pub use auler_domain::review::{Disposition, ReviewItem, ReviewValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewStatus {
    NoChanges,
    Applicable,
    NeedsReview,
    ValidationFailed,
    PartiallyApplied,
    PartiallyAppliedClosed,
    Applied,
    Rejected,
    Cancelled,
    BaseChanged,
}

impl ReviewStatus {
    pub fn is_complete(self) -> bool {
        matches!(
            self,
            Self::NoChanges
                | Self::Applied
                | Self::PartiallyAppliedClosed
                | Self::Rejected
                | Self::Cancelled
        )
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::NoChanges => "no_changes",
            Self::Applicable => "applicable",
            Self::NeedsReview => "needs_review",
            Self::ValidationFailed => "validation_failed",
            Self::PartiallyApplied => "partially_applied",
            Self::PartiallyAppliedClosed => "partially_applied_closed",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::BaseChanged => "base_changed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApprovalBlocker {
    Completed(ReviewStatus),
    Finalized(TermKey),
    BaseChanged { expected: u64, current: u64 },
    DocumentIssues(Vec<Issue>),
    MissingSetup(Vec<Issue>),
    NoApplicableRows,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionReason {
    Unchanged,
    Validation(Vec<Issue>),
    MissingSetup,
    MissingPrerequisites {
        paths: Vec<String>,
        issues: Vec<Issue>,
    },
    ChangedProposal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionIssue {
    pub path: String,
    pub reason: SelectionReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewDecision {
    Rejected,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowDecision {
    Applied {
        revision: u64,
    },
    Rejected {
        actor: String,
        at_ms: i64,
    },
    Undone {
        revision: u64,
        applied_revision: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DraftItem {
    pub item: ReviewItem,
    pub decision: Option<RowDecision>,
    /// Current document path if the historical row still appears in the revised input.
    pub active_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewClosure {
    pub decision: ReviewDecision,
    pub actor: String,
    pub at_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewDraft {
    pub id: i64,
    pub term: TermKey,
    pub origin: Origin,
    pub submitted_by: String,
    pub summary: String,
    pub submitted_at_ms: i64,
    pub submission_id: Option<String>,
    pub submitted_revision: u64,
    pub base_revision: u64,
    pub term_finalized: bool,
    pub setup_required: bool,
    /// Increases whenever the saved review changes, independent of the workspace revision.
    pub version: u64,
    pub document: InputDocument,
    pub issues: Vec<Issue>,
    pub items: Vec<DraftItem>,
    pub closure: Option<ReviewClosure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewOverview {
    pub id: i64,
    pub term: TermKey,
    pub origin: Origin,
    pub summary: String,
    pub submitted_at_ms: i64,
    pub base_revision: u64,
    pub counts: ReviewCounts,
    pub closure: Option<ReviewClosure>,
    #[serde(default)]
    pub kinds: Option<ReviewKinds>,
    status: ReviewStatus,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ReviewKinds {
    pub data: bool,
    pub constraints: bool,
    pub placements: bool,
}

impl ReviewKinds {
    fn of(input: &InputDocument) -> Self {
        let constraints = !input.constraints.is_empty()
            || input.unresolved.iter().any(|row| row.what == "constraint");
        let placements = !input.placements.is_empty()
            || input.unresolved.iter().any(|row| row.what == "placement");
        let data = (!constraints && !placements)
            || input.setup.is_some()
            || !input.courses.is_empty()
            || !input.sections.is_empty()
            || !input.instructors.is_empty()
            || !input.groups.is_empty()
            || !input.rooms.is_empty()
            || !input.equipment.is_empty()
            || input
                .unresolved
                .iter()
                .any(|row| !matches!(row.what.as_str(), "constraint" | "placement"));
        Self {
            data,
            constraints,
            placements,
        }
    }
}

impl ReviewOverview {
    pub fn status(&self, workspace_revision: u64) -> ReviewStatus {
        if !self.status.is_complete() && workspace_revision != self.base_revision {
            ReviewStatus::BaseChanged
        } else {
            self.status
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ReviewCounts {
    pub new: usize,
    pub changed: usize,
    pub unchanged: usize,
    pub needs_review: usize,
    pub errors: usize,
    pub applied: usize,
    pub rejected: usize,
}

impl ReviewDraft {
    pub fn counts(&self) -> ReviewCounts {
        let mut counts = ReviewCounts {
            errors: self.issues.len(),
            ..ReviewCounts::default()
        };
        for row in &self.items {
            match row.decision {
                Some(RowDecision::Applied { .. }) => counts.applied += 1,
                Some(RowDecision::Rejected { .. }) => counts.rejected += 1,
                Some(RowDecision::Undone { .. }) => {}
                None => match row.item.disposition() {
                    Disposition::New => counts.new += 1,
                    Disposition::Update => counts.changed += 1,
                    Disposition::Unchanged => counts.unchanged += 1,
                    Disposition::NeedsReview => counts.needs_review += 1,
                    Disposition::Error => counts.errors += 1,
                },
            }
        }
        counts
    }

    pub fn approval_blocker(&self, workspace_revision: u64) -> Option<ApprovalBlocker> {
        match self.status(workspace_revision) {
            status @ (ReviewStatus::Applied
            | ReviewStatus::PartiallyAppliedClosed
            | ReviewStatus::Rejected
            | ReviewStatus::Cancelled) => {
                return Some(ApprovalBlocker::Completed(status));
            }
            ReviewStatus::BaseChanged => {
                return Some(ApprovalBlocker::BaseChanged {
                    expected: self.base_revision,
                    current: workspace_revision,
                });
            }
            ReviewStatus::NoChanges => return Some(ApprovalBlocker::NoApplicableRows),
            _ => {}
        }
        if self.term_finalized {
            return Some(ApprovalBlocker::Finalized(self.term));
        }
        if !self.issues.is_empty() {
            return Some(ApprovalBlocker::DocumentIssues(self.issues.clone()));
        }
        if self.setup_required {
            let setup = self
                .items
                .iter()
                .find(|row| row.item.path == "setup" && row.decision.is_none());
            if setup.is_none_or(|row| row.item.disposition() != Disposition::New) {
                return Some(ApprovalBlocker::MissingSetup(
                    setup.map(|row| row.item.issues.clone()).unwrap_or_default(),
                ));
            }
        }
        if !self.items.iter().any(|row| {
            row.decision.is_none()
                && matches!(
                    row.item.disposition(),
                    Disposition::New | Disposition::Update
                )
        }) {
            return Some(ApprovalBlocker::NoApplicableRows);
        }
        None
    }

    pub fn status(&self, workspace_revision: u64) -> ReviewStatus {
        if let Some(closure) = &self.closure {
            if self
                .items
                .iter()
                .any(|row| matches!(row.decision, Some(RowDecision::Applied { .. })))
            {
                return ReviewStatus::PartiallyAppliedClosed;
            }
            return match closure.decision {
                ReviewDecision::Rejected => ReviewStatus::Rejected,
                ReviewDecision::Cancelled => ReviewStatus::Cancelled,
            };
        }
        let counts = self.counts();
        let applicable = counts.new + counts.changed > 0;
        if !applicable && counts.needs_review == 0 && counts.errors == 0 {
            return match (counts.applied > 0, counts.rejected > 0) {
                (true, true) => ReviewStatus::PartiallyAppliedClosed,
                (true, false) => ReviewStatus::Applied,
                (false, true) => ReviewStatus::Rejected,
                (false, false) => ReviewStatus::NoChanges,
            };
        }
        if workspace_revision != self.base_revision {
            return ReviewStatus::BaseChanged;
        }
        if counts.applied > 0 {
            ReviewStatus::PartiallyApplied
        } else if counts.errors > 0 {
            ReviewStatus::ValidationFailed
        } else if counts.needs_review > 0 {
            ReviewStatus::NeedsReview
        } else if applicable {
            ReviewStatus::Applicable
        } else {
            ReviewStatus::NoChanges
        }
    }
}

impl Workspace {
    pub fn create_review(
        &self,
        term: TermKey,
        input: InputDocument,
        meta: ChangeMeta,
    ) -> Result<ReviewDraft> {
        self.writer
            .call(move |w| w.create_review(term, input, meta))
    }

    pub fn review(&self, id: i64) -> Result<ReviewDraft> {
        load(&self.reader, id)
    }

    pub fn original_review_input(&self, id: i64) -> Result<InputDocument> {
        read::body(
            &self.reader,
            "SELECT submission FROM review_drafts WHERE id = ?1",
            [id],
        )?
        .ok_or(StoreError::UnknownReview(id))
    }

    pub fn reviews(&self) -> Result<Vec<ReviewOverview>> {
        let mut rows: Vec<ReviewOverview> = read::bodies(
            &self.reader,
            "SELECT overview FROM review_drafts ORDER BY id DESC",
            [],
        )?;
        for row in &mut rows {
            if row.kinds.is_none() {
                row.kinds = Some(ReviewKinds::of(&load(&self.reader, row.id)?.document));
            }
        }
        Ok(rows)
    }

    /// Pass a corrected document, or `None` to compare the same document with the latest data.
    pub fn refresh_review(
        &self,
        id: i64,
        version: u64,
        input: Option<InputDocument>,
    ) -> Result<ReviewDraft> {
        self.writer.call(move |w| {
            let mut draft = load(&w.conn, id)?;
            editable(&draft, version)?;
            if let Some(input) = input {
                track_decided_rows(&mut draft, &input)?;
                draft.document = input;
            }
            draft.base_revision = read::revision(&w.conn)?;
            revalidate(&w.conn, &mut draft)?;
            save(&w.conn, &mut draft)?;
            Ok(draft)
        })
    }

    pub fn approve_review(
        &self,
        id: i64,
        version: u64,
        paths: Vec<String>,
        actor: String,
        request_id: String,
    ) -> Result<Applied> {
        self.writer
            .call(move |w| w.approve_review(id, version, paths, actor, request_id))
    }

    pub fn reject_review_items(
        &self,
        id: i64,
        version: u64,
        paths: Vec<String>,
        actor: String,
    ) -> Result<ReviewDraft> {
        self.writer.call(move |w| {
            if actor.trim().is_empty() {
                return Err(StoreError::ReviewUnavailable);
            }
            let mut draft = load(&w.conn, id)?;
            editable(&draft, version)?;
            let selected = select(&draft, paths)?;
            let at_ms = now_ms();
            for row in &mut draft.items {
                if row.decision.is_none() && selected.contains(&row.item.path) {
                    row.decision = Some(RowDecision::Rejected {
                        actor: actor.clone(),
                        at_ms,
                    });
                }
            }
            if read::revision(&w.conn)? == draft.base_revision {
                revalidate(&w.conn, &mut draft)?;
            }
            save(&w.conn, &mut draft)?;
            Ok(draft)
        })
    }

    pub fn close_review(
        &self,
        id: i64,
        version: u64,
        decision: ReviewDecision,
        actor: String,
    ) -> Result<ReviewDraft> {
        self.writer.call(move |w| {
            if actor.trim().is_empty() {
                return Err(StoreError::ReviewUnavailable);
            }
            let mut draft = load(&w.conn, id)?;
            editable(&draft, version)?;
            draft.closure = Some(ReviewClosure {
                decision,
                actor,
                at_ms: now_ms(),
            });
            save(&w.conn, &mut draft)?;
            Ok(draft)
        })
    }
}

fn load(conn: &Connection, id: i64) -> Result<ReviewDraft> {
    read::body(conn, "SELECT body FROM review_drafts WHERE id = ?1", [id])?
        .ok_or(StoreError::UnknownReview(id))
}

fn save(conn: &Connection, draft: &mut ReviewDraft) -> Result<()> {
    draft.version += 1;
    let overview = ReviewOverview {
        id: draft.id,
        term: draft.term,
        origin: draft.origin,
        summary: draft.summary.clone(),
        submitted_at_ms: draft.submitted_at_ms,
        base_revision: draft.base_revision,
        counts: draft.counts(),
        closure: draft.closure.clone(),
        kinds: Some(ReviewKinds::of(&draft.document)),
        status: draft.status(draft.base_revision),
    };
    conn.execute(
        "UPDATE review_drafts SET body = ?1, overview = ?2 WHERE id = ?3",
        params![
            serde_json::to_string(draft)?,
            serde_json::to_string(&overview)?,
            draft.id
        ],
    )?;
    Ok(())
}

fn editable(draft: &ReviewDraft, version: u64) -> Result<()> {
    if draft.version != version {
        return Err(StoreError::StaleReview);
    }
    if draft.status(draft.base_revision).is_complete() {
        return Err(StoreError::ReviewUnavailable);
    }
    Ok(())
}

fn select(draft: &ReviewDraft, paths: Vec<String>) -> Result<HashSet<String>> {
    let mut selected = HashSet::new();
    if paths.is_empty() {
        return Err(StoreError::ReviewSelection(Vec::new()));
    }
    for path in paths {
        if !draft
            .items
            .iter()
            .any(|r| r.item.path == path && r.decision.is_none())
            || !selected.insert(path.clone())
        {
            return Err(StoreError::ReviewSelection(vec![path]));
        }
    }
    Ok(selected)
}

fn base(conn: &Connection, term: TermKey) -> Result<ReviewBase> {
    match read::load_term(conn, term) {
        Ok(data) => Ok(ReviewBase {
            setup: Some(data.setup),
            equipment: data.equipment,
            rooms: data.rooms,
            instructors: data.instructors,
            groups: data.groups,
            courses: data.courses,
            sections: data.sections,
            constraints: data.constraints,
            placements: data.placements,
        }),
        Err(StoreError::UnknownTerm(_)) => Ok(ReviewBase {
            equipment: read::bodies(conn, "SELECT body FROM equipment ORDER BY id", [])?,
            rooms: read::bodies(conn, "SELECT body FROM rooms ORDER BY id", [])?,
            instructors: read::bodies(conn, "SELECT body FROM instructors ORDER BY id", [])?,
            groups: read::bodies(conn, "SELECT body FROM student_groups ORDER BY id", [])?,
            courses: read::bodies(conn, "SELECT body FROM courses ORDER BY id", [])?,
            ..ReviewBase::default()
        }),
        Err(e) => Err(e),
    }
}

fn id_base(conn: &Connection, doc: &InputDocument) -> Result<i64> {
    let largest: i64 = conn.query_row(
        "SELECT MAX(
            COALESCE((SELECT MAX(id) FROM equipment), 0),
            COALESCE((SELECT MAX(id) FROM rooms), 0),
            COALESCE((SELECT MAX(id) FROM instructors), 0),
            COALESCE((SELECT MAX(id) FROM student_groups), 0),
            COALESCE((SELECT MAX(id) FROM courses), 0),
            COALESCE((SELECT MAX(id) FROM sections), 0),
            COALESCE((SELECT MAX(id) FROM constraints), 0), 0)",
        [],
        |row| row.get(0),
    )?;
    let max_rows = [
        doc.equipment.len(),
        doc.rooms.len(),
        doc.instructors.len(),
        doc.groups.len(),
        doc.courses.len(),
        doc.sections.len(),
        doc.constraints.len(),
    ]
    .into_iter()
    .max()
    .unwrap_or_default();
    if largest.checked_add(max_rows as i64).is_none() {
        return Err(StoreError::Invalid("기록 ID 범위를 초과함".into()));
    }
    Ok(largest)
}

fn target_issue(term: TermKey, actual: Option<TermKey>) -> Option<Issue> {
    (actual != Some(term)).then(|| Issue {
        path: "term".into(),
        kind: IssueKind::InvalidReviewTarget,
        source: None,
    })
}

fn revalidate(conn: &Connection, draft: &mut ReviewDraft) -> Result<()> {
    draft.term_finalized = match read::term_row(conn, draft.term) {
        Ok(term) => !term.editing,
        Err(StoreError::UnknownTerm(_)) => false,
        Err(err) => return Err(err),
    };
    let base = base(conn, draft.term)?;
    draft.setup_required = base.setup.is_none();
    let seed = id_base(conn, &draft.document)?;
    let mut preview = inspect(&draft.document, &base, seed, None)?;
    let decided: HashSet<_> = draft
        .items
        .iter()
        .filter(|r| r.decision.is_some())
        .filter_map(|r| r.active_path.as_deref())
        .collect();
    if !decided.is_empty() {
        let pending = preview
            .items
            .iter()
            .filter(|r| !decided.contains(r.path.as_str()))
            .map(|r| r.path.clone())
            .collect();
        preview = inspect(&draft.document, &base, seed, Some(&pending))?;
    }
    if !preview.issues.iter().any(|issue| issue.path == "term") {
        preview
            .issues
            .extend(target_issue(draft.term, preview.term));
    }
    draft.issues = preview.issues;
    draft.items.retain(|row| row.decision.is_some());
    draft
        .items
        .extend(preview.items.into_iter().map(|item| DraftItem {
            active_path: Some(item.path.clone()),
            item,
            decision: None,
        }));
    draft
        .items
        .sort_by_key(|row| row_order(row.active_path.as_deref().unwrap_or(&row.item.path)));
    Ok(())
}

const COLLECTIONS: &[&str] = &[
    "setup",
    "equipment",
    "rooms",
    "instructors",
    "groups",
    "courses",
    "sections",
    "constraints",
    "placements",
    "unresolved",
];

fn split_path(path: &str) -> (&str, usize) {
    path.strip_suffix(']')
        .and_then(|p| p.split_once('['))
        .and_then(|(name, index)| Some((name, index.parse().ok()?)))
        .unwrap_or((path, 0))
}

fn row_order(path: &str) -> (usize, usize) {
    let (name, index) = split_path(path);
    (
        COLLECTIONS
            .iter()
            .position(|known| *known == name)
            .unwrap_or(COLLECTIONS.len()),
        index,
    )
}

fn track_decided_rows(draft: &mut ReviewDraft, replacement: &InputDocument) -> Result<()> {
    let raw = serde_json::to_value(replacement)?;
    let mut assigned = HashSet::new();
    for row in draft.items.iter_mut().filter(|row| row.decision.is_some()) {
        let (name, index) = split_path(row.active_path.as_deref().unwrap_or(&row.item.path));
        row.active_path = if matches!(row.decision, Some(RowDecision::Undone { .. })) {
            None
        } else if name == "setup" {
            (raw.get(name) == Some(&row.item.input) && assigned.insert(name.to_owned()))
                .then(|| name.to_owned())
        } else {
            raw.get(name)
                .and_then(serde_json::Value::as_array)
                .and_then(|rows| {
                    std::iter::once(index).chain(0..rows.len()).find_map(|pos| {
                        let path = format!("{name}[{pos}]");
                        (rows.get(pos) == Some(&row.item.input) && assigned.insert(path.clone()))
                            .then_some(path)
                    })
                })
        };
    }
    Ok(())
}

fn to_op(value: ReviewValue, actor: &str) -> Op {
    Op::Put(match value {
        ReviewValue::Setup(setup) => return Op::SetSetup(setup),
        ReviewValue::Equipment(item) => Record::Equipment(item),
        ReviewValue::Room(item) => Record::Room(item),
        ReviewValue::Instructor(item) => Record::Instructor(item),
        ReviewValue::Group(item) => Record::Group(item),
        ReviewValue::Course(item) => Record::Course(item),
        ReviewValue::Section(item) => Record::Section(item),
        ReviewValue::Constraint(mut item) => {
            item.confirmed_by =
                (item.status == ConstraintStatus::Confirmed).then(|| actor.to_owned());
            Record::Constraint(item)
        }
        ReviewValue::Placement(item) => Record::Placement(item),
    })
}

pub(crate) struct ReviewStep {
    id: i64,
    pub(crate) applied_revision: u64,
    rows: Vec<ReviewItem>,
}

impl ReviewStep {
    pub(crate) fn update(&self, conn: &Connection, revision: u64, action: Origin) -> Result<()> {
        let mut draft = load(conn, self.id)?;
        for original in &self.rows {
            let decision = (action == Origin::Undo).then_some(RowDecision::Applied {
                revision: self.applied_revision,
            });
            let same_row = |row: &DraftItem| {
                row.item.input == original.input
                    && split_path(&row.item.path).0 == split_path(&original.path).0
            };
            let index = draft.items.iter().position(|row| {
                same_row(row) && action == Origin::Redo && matches!(row.decision,
                    Some(RowDecision::Undone { applied_revision, .. }) if applied_revision == self.applied_revision)
            }).or_else(|| draft.items.iter().position(|row| row.decision == decision && same_row(row)))
                .ok_or(StoreError::StaleReview)?;
            let row = &mut draft.items[index];
            match action {
                Origin::Undo => {
                    row.decision = row.active_path.is_none().then_some(RowDecision::Undone {
                        revision,
                        applied_revision: self.applied_revision,
                    });
                }
                Origin::Redo => {
                    let path = row.item.path.clone();
                    row.item = original.clone();
                    row.item.path = path;
                    row.decision = Some(RowDecision::Applied { revision });
                }
                _ => return Err(StoreError::StaleReview),
            }
        }
        draft.base_revision = revision;
        revalidate(conn, &mut draft)?;
        save(conn, &mut draft)?;
        Ok(())
    }
}

impl Writer {
    fn create_review(
        &mut self,
        term: TermKey,
        input: InputDocument,
        meta: ChangeMeta,
    ) -> Result<ReviewDraft> {
        if !matches!(
            meta.origin,
            Origin::Direct | Origin::Xlsx | Origin::ChatGpt | Origin::Mcp | Origin::Solver
        ) || meta
            .request_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
        {
            return Err(StoreError::ReviewUnavailable);
        }
        let tx = self.conn.transaction()?;
        let submission = serde_json::to_string(&input)?;
        if let Some(request) = &meta.request_id
            && let Some((draft, original)) = tx.query_row(
                "SELECT body, submission FROM review_drafts WHERE origin = ?1 AND request_id = ?2",
                params![meta.origin.as_str(), request],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            ).optional()?
        {
            let draft: ReviewDraft = serde_json::from_str(&draft)?;
            if draft.term != term || draft.submitted_by != meta.actor || original != submission {
                return Err(StoreError::ReviewRequestConflict);
            }
            return Ok(draft);
        }
        let revision = read::revision(&tx)?;
        if let Some(expected) = meta.base_revision
            && expected != revision
        {
            return Err(StoreError::StaleRevision {
                expected,
                current: revision,
            });
        }
        tx.execute(
            "INSERT INTO review_drafts (origin, request_id, submission, body) VALUES (?1, ?2, ?3, '')",
            params![meta.origin.as_str(), meta.request_id, submission],
        )?;
        let mut draft = ReviewDraft {
            id: tx.last_insert_rowid(),
            term,
            origin: meta.origin,
            submitted_by: meta.actor,
            summary: meta.summary,
            submitted_at_ms: now_ms(),
            submission_id: meta.request_id,
            submitted_revision: revision,
            base_revision: revision,
            term_finalized: false,
            setup_required: false,
            version: 0,
            document: input,
            issues: Vec::new(),
            items: Vec::new(),
            closure: None,
        };
        revalidate(&tx, &mut draft)?;
        save(&tx, &mut draft)?;
        tx.commit()?;
        Ok(draft)
    }

    fn approve_review(
        &mut self,
        id: i64,
        version: u64,
        mut paths: Vec<String>,
        actor: String,
        request_id: String,
    ) -> Result<Applied> {
        if actor.trim().is_empty() || request_id.trim().is_empty() {
            return Err(StoreError::ReviewUnavailable);
        }
        paths.sort_unstable();
        let selection = serde_json::to_string(&paths)?;
        let tx = self.conn.transaction()?;
        if let Some((revision, prior_actor, prior_selection)) = tx
            .query_row(
                "SELECT revision, actor, selection FROM review_applications WHERE draft_id = ?1 AND request_id = ?2",
                params![id, request_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
            )
            .optional()?
        {
            if actor != prior_actor || selection != prior_selection {
                return Err(StoreError::ReviewRequestConflict);
            }
            return Ok(Applied {
                revision: revision as u64,
                duplicate: true,
            });
        }
        let mut draft = load(&tx, id)?;
        editable(&draft, version)?;
        let current = read::revision(&tx)?;
        if current != draft.base_revision {
            return Err(StoreError::StaleRevision {
                expected: draft.base_revision,
                current,
            });
        }
        if draft.term_finalized {
            return Err(StoreError::Finalized(draft.term));
        }
        if !draft.issues.is_empty() {
            return Err(StoreError::ReviewDocument(draft.issues.clone()));
        }
        let selected = select(&draft, paths)?;
        let snapshot = base(&tx, draft.term)?;
        if snapshot.setup.is_none() && !selected.contains("setup") {
            return Err(StoreError::ReviewBlocked(vec![SelectionIssue {
                path: "setup".into(),
                reason: SelectionReason::MissingSetup,
            }]));
        }
        let preview = inspect(
            &draft.document,
            &snapshot,
            id_base(&tx, &draft.document)?,
            Some(&selected),
        )?;
        let mut ops = Vec::with_capacity(selected.len());
        let mut rejected = Vec::new();
        for candidate in preview.items {
            let shown = draft
                .items
                .iter()
                .find(|r| r.decision.is_none() && r.item.path == candidate.path);
            let prerequisites: Vec<_> = shown
                .iter()
                .flat_map(|row| &row.item.prerequisites)
                .filter(|path| !selected.contains(*path))
                .cloned()
                .collect();
            if shown.is_some_and(|row| {
                matches!(
                    row.item.disposition(),
                    Disposition::New | Disposition::Update
                ) && row.item.after == candidate.after
            }) && matches!(
                candidate.disposition(),
                Disposition::New | Disposition::Update
            ) && prerequisites.is_empty()
                && let Some(after) = candidate.after
            {
                match after {
                    ReviewValue::Setup(setup) if snapshot.setup.is_none() => {
                        validate_setup(&setup)?;
                        tx.execute(
                            "INSERT INTO terms (year, season, setup) VALUES (?1, ?2, ?3)",
                            params![
                                draft.term.year,
                                draft.term.season.code(),
                                serde_json::to_string(&setup)?
                            ],
                        )?;
                    }
                    _ => ops.push(to_op(after, &actor)),
                }
            } else {
                let reason = if !prerequisites.is_empty() {
                    SelectionReason::MissingPrerequisites {
                        paths: prerequisites,
                        issues: candidate.issues,
                    }
                } else if !candidate.issues.is_empty() {
                    SelectionReason::Validation(candidate.issues)
                } else if let Some(issues) = shown
                    .map(|row| &row.item.issues)
                    .filter(|issues| !issues.is_empty())
                {
                    SelectionReason::Validation(issues.clone())
                } else if candidate.disposition() == Disposition::Unchanged {
                    SelectionReason::Unchanged
                } else {
                    SelectionReason::ChangedProposal
                };
                rejected.push(SelectionIssue {
                    path: candidate.path,
                    reason,
                });
            }
        }
        if !rejected.is_empty() {
            return Err(StoreError::ReviewBlocked(rejected));
        }
        if !read::term_row(&tx, draft.term)?.editing {
            return Err(StoreError::Finalized(draft.term));
        }
        let creates_term = snapshot.setup.is_none();
        let summary = if creates_term {
            "학기 만들기 및 검토 항목 적용"
        } else {
            "검토 항목 적용"
        };
        let meta = ChangeMeta::direct(summary)
            .origin(draft.origin)
            .actor(actor)
            .request(format!("review:{id}:{request_id}"))
            .base(current);
        let (revision, inverse) = record_ops(&tx, Some(draft.term), &ops, &meta)?;
        tx.execute(
            "INSERT INTO review_applications (draft_id, request_id, revision, actor, selection)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, request_id, revision as i64, meta.actor, selection],
        )?;
        let mut undo_rows = Vec::new();
        for row in &mut draft.items {
            if row.decision.is_none() && selected.contains(&row.item.path) {
                row.decision = Some(RowDecision::Applied { revision });
                if !creates_term || row.item.path != "setup" {
                    undo_rows.push(row.item.clone());
                }
            }
        }
        draft.base_revision = revision;
        revalidate(&tx, &mut draft)?;
        save(&tx, &mut draft)?;
        tx.commit()?;
        self.remember_change(
            Some(draft.term),
            ops,
            inverse,
            meta.summary,
            Some(ReviewStep {
                id,
                applied_revision: revision,
                rows: undo_rows,
            }),
        );
        Ok(Applied {
            revision,
            duplicate: false,
        })
    }
}
