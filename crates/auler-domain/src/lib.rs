//! Auler 도메인 모델. 입출력이 없는 순수 코드다.
//!
//! - [`time`]: 요일, 시각, 날짜, 교시표
//! - [`code`]: 학수번호·분반 등 식별 코드 규칙 (항상 문자열, 앞자리 0 보존)
//! - [`model`]: 기준자료, 학기 배치, 편성 조건 (내부 ID로 연결)
//! - [`input`]: 공통 입력 규격 v1 (자연 키로 연결, 원본 위치 포함)
//! - [`resolve`]: 입력 문서 검사와 내부 모델 변환
//! - [`template`]: 표준 XLSX 양식 v1의 시트·열 규격

pub mod code;
pub mod input;
pub mod model;
pub mod resolve;
pub mod template;
pub mod time;
