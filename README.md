# Auler (아울러)

학과 사무실 조교가 강의실 정보와 편성 제약을 반영해 학기 시간표 초안을 만들고, 검토한 결과를 여러 양식으로 내보내는 Windows 11용 경량 로컬 프로그램입니다.

> 개발 초기 단계입니다. 아직 업무에 쓸 수 있는 기능은 없습니다.

## 목표

- 로그인 없이 로컬에서 편성, 저장, 출력
- 표준 XLSX 가져오기, 내장 ChatGPT, 외부 에이전트(MCP) 세 입력 경로와 공통 검토함. AI가 제안하고 조교가 승인하며 편성 엔진이 검증한다.
- 필수·선호 제약, 잠금과 재편성, 확정본 버전 관리
- A4 PDF, XLSX, 학교 제출 양식 출력
- 한국어·영어
- 낮은 메모리 사용량: 브라우저 엔진 없는 네이티브 UI (Rust + [Slint](https://slint.dev))

## 개발 환경

- Windows 11
- Visual Studio Build Tools (C++ 워크로드, Windows SDK)
- Rust stable (`rust-toolchain.toml`)

```powershell
cargo run -p auler                 # 디버그 실행
cargo run -p auler-samples         # samples/와 templates/ 다시 만들기
cargo build --release -p auler     # 릴리스 빌드
cargo test --workspace             # 단위·UI·번역 테스트 (UI 스냅숏: target/tmp/ui-snapshots)
./tools/measure-memory.ps1 -Exe target/release/auler.exe -Label idle   # 유휴 메모리
./tools/measure-interaction.ps1    # 스크롤·끌기 중 CPU·FPS·메모리 (릴리스 빌드 필요)
```

저장소 구성: `app/`(화면), `crates/auler-domain`(도메인 모델, [공통 입력 규격 v1](docs/input-spec-v1.md)), `crates/auler-store`(작업공간 저장소: SQLite, 변경 이력, 확정본), `crates/auler-samples`(가상 데이터, 표준 XLSX 양식 생성), `samples/`(가상 학과 자료), `templates/`(표준 XLSX 양식).

개발용 환경 변수: `AULER_LANG`(ko|en), `AULER_PAGE`(0~7) 시작 언어·화면 고정, `AULER_BENCH`(idle|scroll|drag) 벤치마크 실행 후 종료.

## English

Auler is a lightweight, local-first Windows 11 app that helps university department office assistants draft a semester timetable from room data and scheduling constraints, review it, and export it in several formats. AI assistance (built-in ChatGPT or external agents via MCP) is optional: it proposes, a person approves, and the local scheduling engine validates. The UI is native (Rust + Slint) with no browser engine, to keep memory use low.

This project is in early development and not usable yet.

## 만든 사람·라이선스 / Author and license

- 제작자 / Author: [Lehwon](https://github.com/Lehwon)
- 기술 스택 / Tech stack: Rust, [Slint](https://slint.dev) (소프트웨어 렌더러), Win32 API ([windows-sys](https://github.com/microsoft/windows-rs))
- 라이선스 / License: [MIT](LICENSE)
- 사용한 오픈소스와 라이선스 / Third-party components: [THIRD-PARTY.md](THIRD-PARTY.md). Slint는 Royalty-free License 2.0으로 사용하며 앱의 정보 화면에 표시한다.

앱의 **정보** 화면에서도 같은 내용을 볼 수 있다. / The same information is shown on the app's **About** page.
