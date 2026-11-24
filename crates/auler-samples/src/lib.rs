//! Auler 가상 데이터. 실제 학교·사람과 관계없다.
//!
//! - [`reference::reference`]: 기준 학과 (분반 42, 교원 28, 강의실 12)
//! - [`stress::stress`]: 스트레스 데이터 (분반 150)
//! - [`messy::build`]: 지저분한 원본 엑셀 세 개와 그 올바른 변환 결과
//! - [`template_xlsx::build`]: 표준 XLSX 양식 v1 (한국어판, 영어판)
//!
//! 저장소의 `samples/`와 `templates/`는 [`outputs`]로 만든 파일이다. 테스트가 둘이
//! 같은지 확인하므로, 생성 코드를 바꾸면 `cargo run -p auler-samples`로 다시 만든다.

pub mod messy;
pub mod reference;
pub mod stress;
pub mod template_xlsx;

use std::path::{Path, PathBuf};

pub use reference::reference;
pub use stress::stress;

use auler_domain::input::InputDocument;

fn json(doc: &InputDocument) -> Vec<u8> {
    let mut text = doc.to_json_pretty();
    text.push('\n');
    text.into_bytes()
}

/// 저장소에 두는 생성 파일 전체: (저장소 기준 경로, 내용)
pub fn outputs() -> Result<Vec<(PathBuf, Vec<u8>)>, rust_xlsxwriter::XlsxError> {
    let mut out = vec![
        (PathBuf::from("samples/reference.json"), json(&reference())),
        (PathBuf::from("samples/stress.json"), json(&stress())),
    ];
    let messy = messy::build()?;
    for (name, bytes) in messy.files {
        out.push((Path::new("samples/messy").join(name), bytes));
    }
    out.push((
        PathBuf::from("samples/messy/expected.json"),
        json(&messy.expected),
    ));
    for lang in [template_xlsx::Lang::Ko, template_xlsx::Lang::En] {
        out.push((
            Path::new("templates").join(lang.file_name()),
            template_xlsx::build(lang)?,
        ));
    }
    Ok(out)
}

/// 저장소 최상위 폴더
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
