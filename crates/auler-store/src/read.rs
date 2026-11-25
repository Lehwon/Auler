//! 읽기. 읽기 연결과 쓰기 스레드가 함께 쓴다.

use auler_domain::model::*;
use auler_domain::time::Weekday;
use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;

use crate::error::{Result, StoreError};
use crate::{ChangeRecord, FinalizedInfo, Origin, TermSummary};

pub(crate) struct TermRow {
    pub id: i64,
    pub setup: TermSetup,
    pub editing: bool,
}

pub(crate) fn term_row(conn: &Connection, key: TermKey) -> Result<TermRow> {
    conn.query_row(
        "SELECT id, setup, editing FROM terms WHERE year = ?1 AND season = ?2",
        params![key.year, key.season.code()],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
            ))
        },
    )
    .optional()?
    .ok_or(StoreError::UnknownTerm(key))
    .and_then(|(id, setup, editing)| {
        Ok(TermRow {
            id,
            setup: serde_json::from_str(&setup)?,
            editing,
        })
    })
}

pub(crate) fn term_key_of(conn: &Connection, term_id: i64) -> Result<TermKey> {
    let setup: String =
        conn.query_row("SELECT setup FROM terms WHERE id = ?1", [term_id], |r| {
            r.get(0)
        })?;
    Ok(serde_json::from_str::<TermSetup>(&setup)?.key)
}

/// 본문 열을 읽어 모델로 바꾼다
pub(crate) fn bodies<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<T>> {
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map(params, |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for body in rows {
        out.push(serde_json::from_str(&body?)?);
    }
    Ok(out)
}

pub(crate) fn body<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Option<T>> {
    let body: Option<String> = conn.query_row(sql, params, |r| r.get(0)).optional()?;
    Ok(match body {
        Some(b) => Some(serde_json::from_str(&b)?),
        None => None,
    })
}

pub(crate) fn placement_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Placement> {
    let day: i64 = r.get(2)?;
    Ok(Placement {
        meeting: MeetingRef {
            section: SectionId(r.get(0)?),
            part: r.get(1)?,
        },
        day: Weekday::from_index(day as usize).unwrap_or(Weekday::Mon),
        start: r.get(3)?,
        room: r.get::<_, Option<i64>>(4)?.map(RoomId),
        locked: r.get(5)?,
    })
}

pub(crate) const PLACEMENT_COLUMNS: &str =
    "p.section_id, p.part, p.day, p.start, p.room_id, p.locked";

pub(crate) fn placements(conn: &Connection, term_id: i64) -> Result<Vec<Placement>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {PLACEMENT_COLUMNS} FROM placements p JOIN sections s ON s.id = p.section_id
         WHERE s.term_id = ?1 ORDER BY p.section_id, p.part"
    ))?;
    let rows = stmt.query_map([term_id], placement_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 학기의 현재 초안 전체
pub(crate) fn load_term(conn: &Connection, key: TermKey) -> Result<TermData> {
    let term = term_row(conn, key)?;
    Ok(TermData {
        setup: term.setup,
        courses: bodies(conn, "SELECT body FROM courses ORDER BY id", [])?,
        instructors: bodies(conn, "SELECT body FROM instructors ORDER BY id", [])?,
        groups: bodies(conn, "SELECT body FROM student_groups ORDER BY id", [])?,
        equipment: bodies(conn, "SELECT body FROM equipment ORDER BY id", [])?,
        rooms: bodies(conn, "SELECT body FROM rooms ORDER BY id", [])?,
        sections: bodies(
            conn,
            "SELECT body FROM sections WHERE term_id = ?1 ORDER BY id",
            [term.id],
        )?,
        constraints: bodies(
            conn,
            "SELECT body FROM constraints WHERE term_id = ?1 ORDER BY id",
            [term.id],
        )?,
        placements: placements(conn, term.id)?,
    })
}

pub(crate) fn meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()?)
}

/// 작업공간 전체의 변경 번호. 변경할 때마다 1씩 오른다
pub(crate) fn revision(conn: &Connection) -> Result<u64> {
    Ok(
        conn.query_row("SELECT COALESCE(MAX(revision), 0) FROM changes", [], |r| {
            r.get::<_, i64>(0)
        })? as u64,
    )
}

pub(crate) fn terms(conn: &Connection) -> Result<Vec<TermSummary>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.setup, t.editing,
                (SELECT MAX(version) FROM finalized f WHERE f.term_id = t.id),
                (SELECT COUNT(*) FROM sections s WHERE s.term_id = t.id)
         FROM terms t ORDER BY t.year, t.season",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, bool>(1)?,
            r.get::<_, Option<u32>>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (setup, editing, last_finalized, sections) = row?;
        out.push(TermSummary {
            key: serde_json::from_str::<TermSetup>(&setup)?.key,
            editing,
            last_finalized,
            sections: sections as usize,
        });
    }
    out.sort_by_key(|t| t.key);
    Ok(out)
}

/// 최근 변경부터 `limit`개
pub(crate) fn history(
    conn: &Connection,
    term: Option<TermKey>,
    limit: usize,
) -> Result<Vec<ChangeRecord>> {
    let term_id = term
        .map(|key| term_row(conn, key).map(|term| term.id))
        .transpose()?;
    let mut stmt = conn.prepare_cached(
        "SELECT revision, term_id, at, actor, origin, request_id, base_revision, summary
         FROM changes WHERE ?1 IS NULL OR term_id = ?1 ORDER BY revision DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(
        params![term_id, i64::try_from(limit).unwrap_or(i64::MAX)],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, String>(7)?,
            ))
        },
    )?;
    let mut out = Vec::new();
    for row in rows {
        let (revision, term_id, at_ms, actor, origin, request_id, base, summary) = row?;
        out.push(ChangeRecord {
            revision: revision as u64,
            term: term_id.map(|id| term_key_of(conn, id)).transpose()?,
            at_ms,
            actor,
            origin: Origin::parse(&origin).unwrap_or(Origin::System),
            request_id,
            base_revision: base.map(|b| b as u64),
            summary,
        });
    }
    Ok(out)
}

pub(crate) fn finalized_list(conn: &Connection, key: TermKey) -> Result<Vec<FinalizedInfo>> {
    let term = term_row(conn, key)?;
    let mut stmt = conn.prepare_cached(
        "SELECT version, revision, at, actor FROM finalized WHERE term_id = ?1 ORDER BY version",
    )?;
    let rows = stmt.query_map([term.id], |r| {
        Ok(FinalizedInfo {
            term: key,
            version: r.get(0)?,
            revision: r.get::<_, i64>(1)? as u64,
            at_ms: r.get(2)?,
            actor: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn load_finalized(conn: &Connection, key: TermKey, version: u32) -> Result<TermData> {
    let term = term_row(conn, key)?;
    let snapshot: Option<String> = conn
        .query_row(
            "SELECT snapshot FROM finalized WHERE term_id = ?1 AND version = ?2",
            params![term.id, version],
            |r| r.get(0),
        )
        .optional()?;
    let snapshot =
        snapshot.ok_or_else(|| StoreError::Invalid(format!("{key} 확정본 v{version} 없음")))?;
    Ok(serde_json::from_str(&snapshot)?)
}
