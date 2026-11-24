//! 번역 검사.
//!
//! - 화면(.slint)의 모든 @tr 문구에 한국어 번역이 있어야 한다.
//! - 화면에서 쓰는 Slint 표준 위젯 문구(우클릭 메뉴)도 번역이 있어야 한다.
//! - .slint 문자열에 한글을 직접 쓰지 않는다. 앱 문구는 @tr로, 원본 자료는 Rust 모델로 넣는다.

use std::collections::BTreeSet;
use std::path::Path;

/// 화면에 나타나는 Slint 표준 위젯의 번역 문구 (LineEdit·TextEdit 우클릭 메뉴)
const STD_WIDGET_MSGIDS: [&str; 6] = ["Undo", "Redo", "Cut", "Copy", "Paste", "Select All"];

/// .slint에 직접 써도 되는 한글: 언어 선택 목록의 자기 언어 이름
const ALLOWED_HANGUL_LITERALS: [&str; 1] = ["한국어"];

fn ui_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("ui")
}

fn slint_sources() -> Vec<(String, String)> {
    let mut files: Vec<_> = std::fs::read_dir(ui_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "slint"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(p).unwrap())
        })
        .collect()
}

/// `"..."` 문자열 리터럴 하나를 읽는다. 시작 위치는 여는 따옴표.
fn read_string(src: &str, start: usize) -> Option<(String, usize)> {
    let bytes = src.as_bytes();
    if bytes.get(start) != Some(&b'"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = src[start + 1..].char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                if let Some((_, n)) = chars.next() {
                    out.push(match n {
                        'n' => '\n',
                        other => other,
                    });
                }
            }
            '"' => return Some((out, start + 1 + i + 1)),
            _ => out.push(c),
        }
    }
    None
}

fn skip_ws(src: &str, mut i: usize) -> usize {
    while src[i..].starts_with(char::is_whitespace) {
        i += src[i..].chars().next().unwrap().len_utf8();
    }
    i
}

/// @tr("단수" | "복수" % n) 또는 @tr("문구", 인자…)의 msgid 목록
fn tr_msgids(src: &str) -> Vec<(String, Option<String>)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(pos) = src[from..].find("@tr(") {
        let at = skip_ws(src, from + pos + 4);
        if let Some((singular, end)) = read_string(src, at) {
            let next = skip_ws(src, end);
            let plural = if src[next..].starts_with('|') {
                read_string(src, skip_ws(src, next + 1)).map(|(p, _)| p)
            } else {
                None
            };
            found.push((singular, plural));
        }
        from = from + pos + 4;
    }
    found
}

/// .po에서 번역이 비어 있지 않은 msgid 집합
fn translated_msgids(po: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut current: Option<String> = None;
    for line in po.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("msgid ") {
            current = read_string(rest, 0).map(|(s, _)| s);
        } else if line.starts_with("msgstr") {
            let value = line
                .split_once(' ')
                .and_then(|(_, v)| read_string(v, 0))
                .map(|(s, _)| s);
            if let (Some(id), Some(value)) = (&current, value)
                && !id.is_empty()
                && !value.is_empty()
            {
                ids.insert(id.clone());
            }
        }
    }
    ids
}

#[test]
fn every_ui_string_has_a_korean_translation() {
    let po_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../i18n/ko/LC_MESSAGES/auler.po");
    let translated = translated_msgids(&std::fs::read_to_string(po_path).unwrap());

    let mut missing = BTreeSet::new();
    let mut count = 0;
    for (file, src) in slint_sources() {
        for (singular, _plural) in tr_msgids(&src) {
            count += 1;
            if !translated.contains(&singular) {
                missing.insert(format!("{file}: {singular}"));
            }
        }
    }
    for id in STD_WIDGET_MSGIDS {
        if !translated.contains(id) {
            missing.insert(format!("std-widgets: {id}"));
        }
    }
    assert!(count > 50, "@tr 문구를 찾지 못했다 ({count}개)");
    assert!(
        missing.is_empty(),
        "번역 없음:\n{}",
        missing.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn slint_files_have_no_hardcoded_korean() {
    let mut offenders = Vec::new();
    for (file, src) in slint_sources() {
        for (n, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            let mut i = 0;
            while let Some(q) = code[i..].find('"') {
                let Some((literal, end)) = read_string(code, i + q) else {
                    break;
                };
                let has_hangul = literal
                    .chars()
                    .any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c));
                if has_hangul && !ALLOWED_HANGUL_LITERALS.contains(&literal.as_str()) {
                    offenders.push(format!("{file}:{}: \"{literal}\"", n + 1));
                }
                i = end;
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "@tr 없는 한글 문자열:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn parser_reads_plural_and_escaped_strings() {
    let ids = tr_msgids(r#"x: @tr("{n} item" | "{n} items" % n); y: @tr( "Say \"hi\"", a);"#);
    assert_eq!(
        ids,
        vec![
            ("{n} item".to_string(), Some("{n} items".to_string())),
            ("Say \"hi\"".to_string(), None),
        ]
    );
}
