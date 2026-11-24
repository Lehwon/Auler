//! 쓰기. 작업공간마다 이 스레드 하나만 SQLite에 쓴다.
//!
//! 변경은 작업(`Op`) 목록이다. 작업을 적용하기 전에 지금 상태에서 되돌리는 작업을 만들어
//! 두고, 둘 다 변경 이력에 남긴다. 실행 취소는 되돌리는 작업을 새 변경으로 적용한다.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::thread::JoinHandle;

use auler_domain::model::*;
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Result, StoreError};
use crate::read::{self, TermRow};
use crate::{Applied, ChangeMeta, FinalizedInfo, Op, Origin, Record, RecordRef};

/// 실행 취소 기록 상한. 메모리를 일정하게 쓰려고 오래된 것부터 버린다
pub const UNDO_LIMIT: usize = 200;

struct Entry {
    term: Option<TermKey>,
    ops: Vec<Op>,
    inverse: Vec<Op>,
    summary: String,
}

pub(crate) struct Writer {
    conn: Connection,
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

impl Writer {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            undo: VecDeque::new(),
            redo: Vec::new(),
        }
    }

    pub fn init_meta(&mut self, department: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('department', ?1), ('created_at', ?2)",
            params![department, now_ms().to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn existing_request(&self, request_id: &Option<String>) -> Result<Option<u64>> {
        let Some(id) = request_id else {
            return Ok(None);
        };
        Ok(self
            .conn
            .query_row(
                "SELECT revision FROM changes WHERE request_id = ?1",
                [id],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .map(|r| r as u64))
    }

    /// 작업을 한 트랜잭션으로 적용하고 이력에 남긴다. 되돌리는 작업을 돌려준다.
    fn commit_change(
        &mut self,
        term: Option<TermKey>,
        ops: &[Op],
        meta: &ChangeMeta,
    ) -> Result<(Applied, Vec<Op>)> {
        if let Some(revision) = self.existing_request(&meta.request_id)? {
            return Ok((
                Applied {
                    revision,
                    duplicate: true,
                },
                Vec::new(),
            ));
        }
        let tx = self.conn.transaction()?;
        let current = read::revision(&tx)?;
        if let Some(base) = meta.base_revision
            && base != current
        {
            return Err(StoreError::StaleRevision {
                expected: base,
                current,
            });
        }
        let mut term_row = term.map(|k| read::term_row(&tx, k)).transpose()?;
        let mut inverse = Vec::with_capacity(ops.len());
        for op in ops {
            inverse.push(apply_op(&tx, &mut term_row, op)?);
        }
        inverse.reverse();
        let revision = current + 1;
        tx.execute(
            "INSERT INTO changes (revision, term_id, at, actor, origin, request_id, base_revision, summary, ops, inverse)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                revision as i64,
                term_row.as_ref().map(|t| t.id),
                now_ms(),
                meta.actor,
                meta.origin.as_str(),
                meta.request_id,
                meta.base_revision.map(|b| b as i64),
                meta.summary,
                json(&ops)?,
                json(&inverse)?,
            ],
        )?;
        tx.commit()?;
        Ok((
            Applied {
                revision,
                duplicate: false,
            },
            inverse,
        ))
    }

    fn push_undo(&mut self, entry: Entry) {
        if entry.ops.is_empty() {
            return;
        }
        if self.undo.len() == UNDO_LIMIT {
            self.undo.pop_front();
        }
        self.undo.push_back(entry);
    }

    pub fn apply(
        &mut self,
        term: Option<TermKey>,
        ops: Vec<Op>,
        meta: &ChangeMeta,
    ) -> Result<Applied> {
        let (applied, inverse) = self.commit_change(term, &ops, meta)?;
        if !applied.duplicate {
            self.redo.clear();
            self.push_undo(Entry {
                term,
                ops,
                inverse,
                summary: meta.summary.clone(),
            });
        }
        Ok(applied)
    }

    pub fn undo(&mut self, actor: &str) -> Result<Applied> {
        let entry = self.undo.pop_back().ok_or(StoreError::NothingToUndo)?;
        let meta = ChangeMeta {
            actor: actor.to_owned(),
            origin: Origin::Undo,
            request_id: None,
            base_revision: None,
            summary: entry.summary.clone(),
        };
        match self.commit_change(entry.term, &entry.inverse, &meta) {
            Ok((applied, inverse)) => {
                self.redo.push(Entry {
                    term: entry.term,
                    ops: entry.inverse,
                    inverse,
                    summary: entry.summary,
                });
                Ok(applied)
            }
            Err(e) => {
                self.undo.push_back(entry);
                Err(e)
            }
        }
    }

    pub fn redo(&mut self, actor: &str) -> Result<Applied> {
        let entry = self.redo.pop().ok_or(StoreError::NothingToRedo)?;
        let meta = ChangeMeta {
            actor: actor.to_owned(),
            origin: Origin::Redo,
            request_id: None,
            base_revision: None,
            summary: entry.summary.clone(),
        };
        match self.commit_change(entry.term, &entry.inverse, &meta) {
            Ok((applied, inverse)) => {
                self.push_undo(Entry {
                    term: entry.term,
                    ops: entry.inverse,
                    inverse,
                    summary: entry.summary,
                });
                Ok(applied)
            }
            Err(e) => {
                self.redo.push(entry);
                Err(e)
            }
        }
    }

    /// 학기를 만든다. 되돌리기 대상이 아니다
    pub fn create_term(&mut self, setup: &TermSetup, meta: &ChangeMeta) -> Result<Applied> {
        if let Some(revision) = self.existing_request(&meta.request_id)? {
            return Ok(Applied {
                revision,
                duplicate: true,
            });
        }
        match read::term_row(&self.conn, setup.key) {
            Ok(_) => return Err(StoreError::TermExists(setup.key)),
            Err(StoreError::UnknownTerm(_)) => {}
            Err(e) => return Err(e),
        }
        validate_setup(setup)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO terms (year, season, setup) VALUES (?1, ?2, ?3)",
            params![setup.key.year, setup.key.season.code(), json(setup)?],
        )?;
        let term_id = tx.last_insert_rowid();
        let revision = record_plain_change(&tx, Some(term_id), meta)?;
        tx.commit()?;
        Ok(Applied {
            revision,
            duplicate: false,
        })
    }

    /// 검사를 마친 학기 자료를 작업공간에 넣는다. 기준자료는 자연 키(코드, 건물+호실)로
    /// 맞춰 있으면 고치고 없으면 새로 만든다. 학기가 없으면 먼저 만든다. 한 변경으로 남아
    /// 실행 취소할 수 있다.
    pub fn import_term(&mut self, data: &TermData, meta: &ChangeMeta) -> Result<Applied> {
        if let Some(revision) = self.existing_request(&meta.request_id)? {
            return Ok(Applied {
                revision,
                duplicate: true,
            });
        }
        let key = data.setup.key;
        let term_exists = match read::term_row(&self.conn, key) {
            Ok(_) => true,
            Err(StoreError::UnknownTerm(_)) => false,
            Err(e) => return Err(e),
        };
        if !term_exists {
            let create = ChangeMeta {
                request_id: None,
                base_revision: None,
                summary: format!("{}: 학기 만들기", meta.summary),
                ..meta.clone()
            };
            self.create_term(&data.setup, &create)?;
        }
        let term = read::term_row(&self.conn, key)?;
        let ops = plan_import(&self.conn, term.id, data, term_exists)?;
        self.apply(Some(key), ops, meta)
    }

    pub fn finalize(
        &mut self,
        key: TermKey,
        expected: u64,
        meta: &ChangeMeta,
    ) -> Result<FinalizedInfo> {
        if let Some(id) = &meta.request_id {
            let done = self
                .conn
                .query_row(
                    "SELECT version, revision, at, actor FROM finalized WHERE request_id = ?1",
                    [id],
                    |r| {
                        Ok(FinalizedInfo {
                            term: key,
                            version: r.get(0)?,
                            revision: r.get::<_, i64>(1)? as u64,
                            at_ms: r.get(2)?,
                            actor: r.get(3)?,
                        })
                    },
                )
                .optional()?;
            if let Some(done) = done {
                return Ok(done);
            }
        }
        let tx = self.conn.transaction()?;
        let current = read::revision(&tx)?;
        if current != expected {
            return Err(StoreError::StaleRevision { expected, current });
        }
        let term = read::term_row(&tx, key)?;
        if !term.editing {
            return Err(StoreError::Finalized(key));
        }
        let snapshot = read::load_term(&tx, key)?;
        let version: u32 = tx.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM finalized WHERE term_id = ?1",
            [term.id],
            |r| r.get(0),
        )?;
        let at = now_ms();
        let revision = record_plain_change(&tx, Some(term.id), meta)?;
        tx.execute(
            "INSERT INTO finalized (term_id, version, revision, at, actor, request_id, snapshot)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                term.id,
                version,
                revision as i64,
                at,
                meta.actor,
                meta.request_id,
                json(&snapshot)?
            ],
        )?;
        tx.execute("UPDATE terms SET editing = 0 WHERE id = ?1", [term.id])?;
        tx.commit()?;
        // 확정 전의 편집은 되돌릴 수 없다
        self.undo.clear();
        self.redo.clear();
        Ok(FinalizedInfo {
            term: key,
            version,
            revision,
            at_ms: at,
            actor: meta.actor.clone(),
        })
    }

    /// 확정된 학기에 수정용 초안을 연다. 초안은 마지막 확정본과 같은 상태에서 시작한다
    pub fn start_correction(&mut self, key: TermKey, meta: &ChangeMeta) -> Result<Applied> {
        let tx = self.conn.transaction()?;
        let term = read::term_row(&tx, key)?;
        let finalized: i64 = tx.query_row(
            "SELECT COUNT(*) FROM finalized WHERE term_id = ?1",
            [term.id],
            |r| r.get(0),
        )?;
        if term.editing || finalized == 0 {
            return Err(StoreError::NotFinalized(key));
        }
        tx.execute("UPDATE terms SET editing = 1 WHERE id = ?1", [term.id])?;
        let revision = record_plain_change(&tx, Some(term.id), meta)?;
        tx.commit()?;
        Ok(Applied {
            revision,
            duplicate: false,
        })
    }
}

/// 자료를 바꾸지 않는 변경(학기 만들기, 확정, 수정용 초안)을 이력에 남긴다
fn record_plain_change(conn: &Connection, term_id: Option<i64>, meta: &ChangeMeta) -> Result<u64> {
    let revision = read::revision(conn)? + 1;
    conn.execute(
        "INSERT INTO changes (revision, term_id, at, actor, origin, request_id, base_revision, summary, ops, inverse)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '[]', '[]')",
        params![
            revision as i64,
            term_id,
            now_ms(),
            meta.actor,
            meta.origin.as_str(),
            meta.request_id,
            meta.base_revision.map(|b| b as i64),
            meta.summary,
        ],
    )?;
    Ok(revision)
}

fn validate_setup(setup: &TermSetup) -> Result<()> {
    if setup.days.is_empty() {
        return Err(StoreError::Invalid("수업 요일이 없음".into()));
    }
    Ok(())
}

fn editing(term: &Option<TermRow>) -> Result<&TermRow> {
    let t = term
        .as_ref()
        .ok_or_else(|| StoreError::Invalid("학기를 지정해야 하는 변경".into()))?;
    if !t.editing {
        return Err(StoreError::Finalized(t.setup.key));
    }
    Ok(t)
}

fn exists(conn: &Connection, table: &str, id: i64) -> Result<bool> {
    Ok(conn
        .query_row(
            &format!("SELECT 1 FROM {table} WHERE id = ?1"),
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// 같은 키를 다른 기록이 쓰고 있으면 중복
fn ensure_unique(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    id: i64,
    key: impl FnOnce() -> String,
) -> Result<()> {
    let other: Option<i64> = conn.query_row(sql, params, |r| r.get(0)).optional()?;
    match other {
        Some(other) if other != id => Err(StoreError::Duplicate(key())),
        _ => Ok(()),
    }
}

fn all_sections(conn: &Connection) -> Result<Vec<Section>> {
    read::bodies(conn, "SELECT body FROM sections ORDER BY id", [])
}

fn all_constraints(conn: &Connection) -> Result<Vec<Constraint>> {
    read::bodies(conn, "SELECT body FROM constraints ORDER BY id", [])
}

fn targeting(conn: &Connection, target: Target) -> Result<Vec<RecordRef>> {
    Ok(all_constraints(conn)?
        .into_iter()
        .filter(|c| c.target == target)
        .map(|c| RecordRef::Constraint(c.id))
        .collect())
}

fn section_term(conn: &Connection, id: SectionId) -> Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT term_id FROM sections WHERE id = ?1", [id.0], |r| {
            r.get(0)
        })
        .optional()?)
}

fn section_in_term(conn: &Connection, term: &TermRow, id: SectionId) -> Result<Section> {
    match section_term(conn, id)? {
        Some(t) if t == term.id => {}
        _ => return Err(StoreError::Missing(RecordRef::Section(id))),
    }
    read::body(conn, "SELECT body FROM sections WHERE id = ?1", [id.0])?
        .ok_or(StoreError::Missing(RecordRef::Section(id)))
}

fn placements_of(conn: &Connection, section: SectionId) -> Result<Vec<Placement>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {} FROM placements p WHERE p.section_id = ?1 ORDER BY p.part",
        read::PLACEMENT_COLUMNS
    ))?;
    let rows = stmt.query_map([section.0], read::placement_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn check_placement_fits(setup: &TermSetup, split: &[u8], p: &Placement) -> Result<()> {
    let Some(&span) = split.get(p.meeting.part as usize) else {
        return Err(StoreError::Invalid(format!(
            "분반 {:?}에 {}번째 배치 단위가 없음",
            p.meeting.section,
            p.meeting.part + 1
        )));
    };
    if !setup.periods.contains_span(p.start as usize, span as usize) {
        return Err(StoreError::Invalid("배치가 마지막 교시를 넘음".into()));
    }
    if !setup.days.contains(&p.day) {
        return Err(StoreError::Invalid(format!("수업 요일이 아닌 {}", p.day)));
    }
    Ok(())
}

fn check_windows(setup: &TermSetup, rule: &Option<Rule>) -> Result<()> {
    let Some(Rule::AvoidTimes { windows }) = rule else {
        return Ok(());
    };
    for w in windows {
        if let TimeWindow::Periods { first, last, .. } = *w
            && (first > last || last as usize >= setup.periods.len())
        {
            return Err(StoreError::Invalid("조건의 교시가 교시표 밖".into()));
        }
    }
    Ok(())
}

fn check_target(conn: &Connection, term: &TermRow, target: Target) -> Result<()> {
    let ok = match target {
        Target::Instructor(id) => exists(conn, "instructors", id.0)?,
        Target::Group(id) => exists(conn, "student_groups", id.0)?,
        Target::Room(id) => exists(conn, "rooms", id.0)?,
        Target::Section(id) => section_term(conn, id)? == Some(term.id),
    };
    if ok {
        Ok(())
    } else {
        Err(StoreError::Invalid(format!("조건 대상 {target:?} 없음")))
    }
}

fn apply_op(conn: &Connection, term: &mut Option<TermRow>, op: &Op) -> Result<Op> {
    match op {
        Op::Put(record) => put(conn, term, record),
        Op::Remove(r) => remove(conn, term, *r),
        Op::SetSetup(setup) => set_setup(conn, term, setup),
    }
}

fn put(conn: &Connection, term: &mut Option<TermRow>, record: &Record) -> Result<Op> {
    macro_rules! upsert_coded {
        ($table:literal, $variant:ident, $ref:ident, $item:expr, $label:literal) => {{
            let item = $item;
            let old = read::body(
                conn,
                concat!("SELECT body FROM ", $table, " WHERE id = ?1"),
                [item.id.0],
            )?;
            ensure_unique(
                conn,
                concat!("SELECT id FROM ", $table, " WHERE code = ?1"),
                [item.code.as_str()],
                item.id.0,
                || format!(concat!($label, " {}"), item.code),
            )?;
            conn.execute(
                concat!(
                    "INSERT INTO ",
                    $table,
                    " (id, code, body) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET code = excluded.code, body = excluded.body"
                ),
                params![item.id.0, item.code.as_str(), json(item)?],
            )?;
            Ok(match old {
                Some(old) => Op::Put(Record::$variant(old)),
                None => Op::Remove(RecordRef::$ref(item.id)),
            })
        }};
    }
    match record {
        Record::Course(c) => upsert_coded!("courses", Course, Course, c, "학수번호"),
        Record::Instructor(t) => {
            upsert_coded!("instructors", Instructor, Instructor, t, "교원 코드")
        }
        Record::Group(g) => upsert_coded!("student_groups", Group, Group, g, "수강 집단 코드"),
        Record::Equipment(e) => upsert_coded!("equipment", Equipment, Equipment, e, "장비 코드"),
        Record::Room(r) => {
            for e in &r.equipment {
                if !exists(conn, "equipment", e.equipment.0)? {
                    return Err(StoreError::Missing(RecordRef::Equipment(e.equipment)));
                }
            }
            let old = read::body(conn, "SELECT body FROM rooms WHERE id = ?1", [r.id.0])?;
            ensure_unique(
                conn,
                "SELECT id FROM rooms WHERE building = ?1 AND number = ?2",
                params![r.key.building, r.key.number],
                r.id.0,
                || format!("강의실 {}", r.key),
            )?;
            conn.execute(
                "INSERT INTO rooms (id, building, number, body) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET building = excluded.building, number = excluded.number, body = excluded.body",
                params![r.id.0, r.key.building, r.key.number, json(r)?],
            )?;
            Ok(match old {
                Some(old) => Op::Put(Record::Room(old)),
                None => Op::Remove(RecordRef::Room(r.id)),
            })
        }
        Record::Section(s) => {
            let t = editing(term)?;
            if !exists(conn, "courses", s.course.0)? {
                return Err(StoreError::Missing(RecordRef::Course(s.course)));
            }
            for id in &s.instructors {
                if !exists(conn, "instructors", id.0)? {
                    return Err(StoreError::Missing(RecordRef::Instructor(*id)));
                }
            }
            for g in &s.groups {
                if !exists(conn, "student_groups", g.group.0)? {
                    return Err(StoreError::Missing(RecordRef::Group(g.group)));
                }
            }
            for e in &s.equipment {
                if !exists(conn, "equipment", e.equipment.0)? {
                    return Err(StoreError::Missing(RecordRef::Equipment(e.equipment)));
                }
            }
            if s.split.is_empty()
                || s.split
                    .iter()
                    .any(|&n| n == 0 || n as usize > t.setup.periods.len())
            {
                return Err(StoreError::Invalid(
                    "분할이 비었거나 하루 교시 수를 넘음".into(),
                ));
            }
            let old: Option<Section> = match section_term(conn, s.id)? {
                Some(other) if other != t.id => {
                    return Err(StoreError::Invalid("다른 학기의 분반".into()));
                }
                Some(_) => read::body(conn, "SELECT body FROM sections WHERE id = ?1", [s.id.0])?,
                None => None,
            };
            for p in placements_of(conn, s.id)? {
                check_placement_fits(&t.setup, &s.split, &p)?;
            }
            ensure_unique(
                conn,
                "SELECT id FROM sections WHERE term_id = ?1 AND course_id = ?2 AND code = ?3",
                params![t.id, s.course.0, s.code.as_str()],
                s.id.0,
                || format!("분반 {}", s.code),
            )?;
            conn.execute(
                "INSERT INTO sections (id, term_id, course_id, code, body) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET course_id = excluded.course_id, code = excluded.code, body = excluded.body",
                params![s.id.0, t.id, s.course.0, s.code.as_str(), json(s)?],
            )?;
            Ok(match old {
                Some(old) => Op::Put(Record::Section(old)),
                None => Op::Remove(RecordRef::Section(s.id)),
            })
        }
        Record::Constraint(c) => {
            let t = editing(term)?;
            check_target(conn, t, c.target)?;
            check_windows(&t.setup, &c.rule)?;
            let old_term: Option<i64> = conn
                .query_row(
                    "SELECT term_id FROM constraints WHERE id = ?1",
                    [c.id.0],
                    |r| r.get(0),
                )
                .optional()?;
            if old_term.is_some_and(|o| o != t.id) {
                return Err(StoreError::Invalid("다른 학기의 조건".into()));
            }
            let old = read::body(conn, "SELECT body FROM constraints WHERE id = ?1", [c.id.0])?;
            conn.execute(
                "INSERT INTO constraints (id, term_id, body) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET body = excluded.body",
                params![c.id.0, t.id, json(c)?],
            )?;
            Ok(match old {
                Some(old) => Op::Put(Record::Constraint(old)),
                None => Op::Remove(RecordRef::Constraint(c.id)),
            })
        }
        Record::Placement(p) => {
            let t = editing(term)?;
            let section = section_in_term(conn, t, p.meeting.section)?;
            check_placement_fits(&t.setup, &section.split, p)?;
            if let Some(room) = p.room
                && !exists(conn, "rooms", room.0)?
            {
                return Err(StoreError::Missing(RecordRef::Room(room)));
            }
            let old = placements_of(conn, p.meeting.section)?
                .into_iter()
                .find(|o| o.meeting == p.meeting);
            conn.execute(
                "INSERT INTO placements (section_id, part, day, start, room_id, locked) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(section_id, part) DO UPDATE SET day = excluded.day, start = excluded.start,
                 room_id = excluded.room_id, locked = excluded.locked",
                params![
                    p.meeting.section.0,
                    p.meeting.part,
                    p.day.index() as i64,
                    p.start,
                    p.room.map(|r| r.0),
                    p.locked
                ],
            )?;
            Ok(match old {
                Some(old) => Op::Put(Record::Placement(old)),
                None => Op::Remove(RecordRef::Placement(p.meeting)),
            })
        }
    }
}

fn remove(conn: &Connection, term: &mut Option<TermRow>, r: RecordRef) -> Result<Op> {
    let in_use = |used_by: Vec<RecordRef>| -> Result<()> {
        if used_by.is_empty() {
            Ok(())
        } else {
            Err(StoreError::InUse { what: r, used_by })
        }
    };
    let sections_using = |f: &dyn Fn(&Section) -> bool| -> Result<Vec<RecordRef>> {
        Ok(all_sections(conn)?
            .into_iter()
            .filter(|s| f(s))
            .map(|s| RecordRef::Section(s.id))
            .collect())
    };
    macro_rules! delete {
        ($table:literal, $variant:ident, $id:expr) => {{
            let old = read::body(
                conn,
                concat!("SELECT body FROM ", $table, " WHERE id = ?1"),
                [$id],
            )?
            .ok_or(StoreError::Missing(r))?;
            conn.execute(concat!("DELETE FROM ", $table, " WHERE id = ?1"), [$id])?;
            Ok(Op::Put(Record::$variant(old)))
        }};
    }
    match r {
        RecordRef::Course(id) => {
            in_use(sections_using(&|s| s.course == id)?)?;
            delete!("courses", Course, id.0)
        }
        RecordRef::Instructor(id) => {
            let mut used = sections_using(&|s| s.instructors.contains(&id))?;
            used.extend(targeting(conn, Target::Instructor(id))?);
            in_use(used)?;
            delete!("instructors", Instructor, id.0)
        }
        RecordRef::Group(id) => {
            let mut used = sections_using(&|s| s.groups.iter().any(|g| g.group == id))?;
            used.extend(targeting(conn, Target::Group(id))?);
            in_use(used)?;
            delete!("student_groups", Group, id.0)
        }
        RecordRef::Equipment(id) => {
            let rooms: Vec<Room> = read::bodies(conn, "SELECT body FROM rooms ORDER BY id", [])?;
            let mut used: Vec<RecordRef> = rooms
                .into_iter()
                .filter(|room| room.equipment.iter().any(|e| e.equipment == id))
                .map(|room| RecordRef::Room(room.id))
                .collect();
            used.extend(sections_using(&|s| {
                s.equipment.iter().any(|e| e.equipment == id)
            })?);
            in_use(used)?;
            delete!("equipment", Equipment, id.0)
        }
        RecordRef::Room(id) => {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {} FROM placements p WHERE p.room_id = ?1",
                read::PLACEMENT_COLUMNS
            ))?;
            let mut used: Vec<RecordRef> = stmt
                .query_map([id.0], read::placement_from_row)?
                .map(|p| p.map(|p| RecordRef::Placement(p.meeting)))
                .collect::<rusqlite::Result<_>>()?;
            used.extend(targeting(conn, Target::Room(id))?);
            in_use(used)?;
            delete!("rooms", Room, id.0)
        }
        RecordRef::Section(id) => {
            let t = editing(term)?;
            section_in_term(conn, t, id)?;
            let mut used: Vec<RecordRef> = placements_of(conn, id)?
                .into_iter()
                .map(|p| RecordRef::Placement(p.meeting))
                .collect();
            used.extend(targeting(conn, Target::Section(id))?);
            in_use(used)?;
            delete!("sections", Section, id.0)
        }
        RecordRef::Constraint(id) => {
            let t = editing(term)?;
            let owner: Option<i64> = conn
                .query_row(
                    "SELECT term_id FROM constraints WHERE id = ?1",
                    [id.0],
                    |r| r.get(0),
                )
                .optional()?;
            if owner != Some(t.id) {
                return Err(StoreError::Missing(r));
            }
            delete!("constraints", Constraint, id.0)
        }
        RecordRef::Placement(meeting) => {
            let t = editing(term)?;
            section_in_term(conn, t, meeting.section)?;
            let old = placements_of(conn, meeting.section)?
                .into_iter()
                .find(|p| p.meeting == meeting)
                .ok_or(StoreError::Missing(r))?;
            conn.execute(
                "DELETE FROM placements WHERE section_id = ?1 AND part = ?2",
                params![meeting.section.0, meeting.part],
            )?;
            Ok(Op::Put(Record::Placement(old)))
        }
    }
}

fn set_setup(conn: &Connection, term: &mut Option<TermRow>, setup: &TermSetup) -> Result<Op> {
    let t = editing(term)?;
    if setup.key != t.setup.key {
        return Err(StoreError::Invalid("학기 키는 바꿀 수 없음".into()));
    }
    validate_setup(setup)?;
    let sections: Vec<Section> =
        read::bodies(conn, "SELECT body FROM sections WHERE term_id = ?1", [t.id])?;
    let split: HashMap<SectionId, &[u8]> = sections
        .iter()
        .map(|s| (s.id, s.split.as_slice()))
        .collect();
    for p in read::placements(conn, t.id)? {
        let parts = split.get(&p.meeting.section).copied().unwrap_or_default();
        check_placement_fits(setup, parts, &p)?;
    }
    for s in &sections {
        if s.split.iter().any(|&n| n as usize > setup.periods.len()) {
            return Err(StoreError::Invalid(
                "교시 수보다 긴 분할 단위가 있음".into(),
            ));
        }
    }
    let constraints: Vec<Constraint> = read::bodies(
        conn,
        "SELECT body FROM constraints WHERE term_id = ?1",
        [t.id],
    )?;
    for c in &constraints {
        check_windows(setup, &c.rule)?;
    }
    conn.execute(
        "UPDATE terms SET setup = ?1 WHERE id = ?2",
        params![json(setup)?, t.id],
    )?;
    let old = std::mem::replace(
        &mut term.as_mut().expect("위에서 확인").setup,
        setup.clone(),
    );
    Ok(Op::SetSetup(old))
}

/// 가져오기 계획: 문서 안의 ID를 작업공간 ID로 바꾼 작업 목록
fn plan_import(
    conn: &Connection,
    term_id: i64,
    data: &TermData,
    term_exists: bool,
) -> Result<Vec<Op>> {
    let mut next: HashMap<&str, i64> = HashMap::new();
    for table in [
        "courses",
        "instructors",
        "student_groups",
        "equipment",
        "rooms",
        "sections",
        "constraints",
    ] {
        let max: i64 = conn.query_row(
            &format!("SELECT COALESCE(MAX(id), 0) FROM {table}"),
            [],
            |r| r.get(0),
        )?;
        next.insert(table, max + 1);
    }
    let mut alloc = |table: &'static str| {
        let id = next[table];
        next.insert(table, id + 1);
        id
    };
    let find = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> Result<Option<i64>> {
        Ok(conn.query_row(sql, params, |r| r.get(0)).optional()?)
    };

    let mut ops = Vec::new();
    if term_exists {
        ops.push(Op::SetSetup(data.setup.clone()));
    }

    let mut equipment = HashMap::new();
    for e in &data.equipment {
        let id = find(
            "SELECT id FROM equipment WHERE code = ?1",
            &[&e.code.as_str()],
        )?
        .unwrap_or_else(|| alloc("equipment"));
        equipment.insert(e.id, EquipmentId(id));
        ops.push(Op::Put(Record::Equipment(EquipmentKind {
            id: EquipmentId(id),
            ..e.clone()
        })));
    }
    let eq = |id: EquipmentId| {
        equipment
            .get(&id)
            .copied()
            .ok_or(StoreError::Missing(RecordRef::Equipment(id)))
    };

    let mut rooms = HashMap::new();
    for r in &data.rooms {
        let id = find(
            "SELECT id FROM rooms WHERE building = ?1 AND number = ?2",
            &[&r.key.building, &r.key.number],
        )?
        .unwrap_or_else(|| alloc("rooms"));
        rooms.insert(r.id, RoomId(id));
        let mut room = r.clone();
        room.id = RoomId(id);
        for e in &mut room.equipment {
            e.equipment = eq(e.equipment)?;
        }
        ops.push(Op::Put(Record::Room(room)));
    }

    let mut instructors = HashMap::new();
    for t in &data.instructors {
        let id = find(
            "SELECT id FROM instructors WHERE code = ?1",
            &[&t.code.as_str()],
        )?
        .unwrap_or_else(|| alloc("instructors"));
        instructors.insert(t.id, InstructorId(id));
        ops.push(Op::Put(Record::Instructor(Instructor {
            id: InstructorId(id),
            ..t.clone()
        })));
    }

    let mut groups = HashMap::new();
    for g in &data.groups {
        let id = find(
            "SELECT id FROM student_groups WHERE code = ?1",
            &[&g.code.as_str()],
        )?
        .unwrap_or_else(|| alloc("student_groups"));
        groups.insert(g.id, GroupId(id));
        ops.push(Op::Put(Record::Group(StudentGroup {
            id: GroupId(id),
            ..g.clone()
        })));
    }

    let mut courses = HashMap::new();
    for c in &data.courses {
        let id = find(
            "SELECT id FROM courses WHERE code = ?1",
            &[&c.code.as_str()],
        )?
        .unwrap_or_else(|| alloc("courses"));
        courses.insert(c.id, CourseId(id));
        ops.push(Op::Put(Record::Course(Course {
            id: CourseId(id),
            ..c.clone()
        })));
    }

    let mut sections = HashMap::new();
    for s in &data.sections {
        let course = *courses
            .get(&s.course)
            .ok_or(StoreError::Missing(RecordRef::Course(s.course)))?;
        let id = find(
            "SELECT id FROM sections WHERE term_id = ?1 AND course_id = ?2 AND code = ?3",
            &[&term_id, &course.0, &s.code.as_str()],
        )?
        .unwrap_or_else(|| alloc("sections"));
        sections.insert(s.id, SectionId(id));
        let mut section = s.clone();
        section.id = SectionId(id);
        section.course = course;
        section.instructors = s
            .instructors
            .iter()
            .map(|t| {
                instructors
                    .get(t)
                    .copied()
                    .ok_or(StoreError::Missing(RecordRef::Instructor(*t)))
            })
            .collect::<Result<_>>()?;
        for g in &mut section.groups {
            g.group = *groups
                .get(&g.group)
                .ok_or(StoreError::Missing(RecordRef::Group(g.group)))?;
        }
        for e in &mut section.equipment {
            e.equipment = eq(e.equipment)?;
        }
        ops.push(Op::Put(Record::Section(section)));
    }

    for c in &data.constraints {
        let target = match c.target {
            Target::Instructor(t) => Target::Instructor(
                *instructors
                    .get(&t)
                    .ok_or(StoreError::Missing(RecordRef::Instructor(t)))?,
            ),
            Target::Section(s) => Target::Section(
                *sections
                    .get(&s)
                    .ok_or(StoreError::Missing(RecordRef::Section(s)))?,
            ),
            Target::Group(g) => Target::Group(
                *groups
                    .get(&g)
                    .ok_or(StoreError::Missing(RecordRef::Group(g)))?,
            ),
            Target::Room(r) => Target::Room(
                *rooms
                    .get(&r)
                    .ok_or(StoreError::Missing(RecordRef::Room(r)))?,
            ),
        };
        let id = ConstraintId(alloc("constraints"));
        ops.push(Op::Put(Record::Constraint(Constraint {
            id,
            target,
            ..c.clone()
        })));
    }

    for p in &data.placements {
        let section = *sections
            .get(&p.meeting.section)
            .ok_or(StoreError::Missing(RecordRef::Section(p.meeting.section)))?;
        let room = p
            .room
            .map(|r| {
                rooms
                    .get(&r)
                    .copied()
                    .ok_or(StoreError::Missing(RecordRef::Room(r)))
            })
            .transpose()?;
        ops.push(Op::Put(Record::Placement(Placement {
            meeting: MeetingRef {
                section,
                part: p.meeting.part,
            },
            room,
            ..p.clone()
        })));
    }
    Ok(ops)
}

type Job = Box<dyn FnOnce(&mut Writer) + Send>;

/// 쓰기 스레드 손잡이. 놓으면 남은 작업을 마치고 스레드를 끝낸다
pub(crate) struct WriterHandle {
    tx: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl WriterHandle {
    pub fn spawn(mut writer: Writer) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
            .name("auler-store-writer".into())
            .spawn(move || {
                for job in rx {
                    job(&mut writer);
                }
            })?;
        Ok(Self {
            tx: Some(tx),
            thread: Some(thread),
        })
    }

    /// 쓰기 스레드에서 `f`를 실행하고 결과를 기다린다
    pub fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Writer) -> Result<R> + Send + 'static,
    ) -> Result<R> {
        let (rtx, rrx) = mpsc::channel();
        let job: Job = Box::new(move |w| {
            let _ = rtx.send(f(w));
        });
        self.tx
            .as_ref()
            .ok_or(StoreError::WriterGone)?
            .send(job)
            .map_err(|_| StoreError::WriterGone)?;
        rrx.recv().map_err(|_| StoreError::WriterGone)?
    }
}

impl Drop for WriterHandle {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
