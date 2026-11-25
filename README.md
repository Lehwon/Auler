# Auler (아울러)

학과 사무실 조교가 강의실 정보와 편성 제약을 반영해 학기 시간표 초안을 만들고, 검토한 결과를 여러 양식으로 내보내는 Windows 11용 경량 로컬 프로그램입니다.

> 개발 중입니다. 로컬 작업공간에서 표준 XLSX 가져오기, 수동·자동 편성, 검토와 확정을 거쳐 PDF·Excel·인계 파일을 만들 수 있습니다.

[Windows 설치 안내 / Windows installation guide](docs/install.md)

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
cargo run --release -p auler-cli -- samples/reference.json  # 가상 학과 검증과 자동 편성
cargo build --release -p auler     # 릴리스 빌드
cargo test --workspace             # 단위·UI·번역 테스트 (UI 스냅숏: target/tmp/ui-snapshots)
./tools/measure-memory.ps1 -Exe target/release/auler.exe -Label idle   # 유휴 메모리
./tools/measure-interaction.ps1    # 스크롤·끌기 중 CPU·FPS·메모리 (릴리스 빌드 필요)
```

저장소 구성: `app/`(화면), `crates/auler-domain`(도메인 모델, [공통 입력 규격 v1](docs/input-spec-v1.md)), `crates/auler-store`(작업공간, 변경 이력, 검토 초안, 확정본), `crates/auler-io`(표준 XLSX 가져오기, 확정본 출력), `crates/auler-engine`(배치 검증 및 자동 편성), `crates/auler-cli`(명령줄 검증 및 편성), `crates/auler-samples`(가상 데이터, 표준 XLSX 양식 생성), `samples/`(가상 학과 자료), `templates/`(표준 XLSX 양식).

개발용 환경 변수: `AULER_LANG`(ko|en), `AULER_PAGE`(0~7) 시작 언어·화면 고정, `AULER_BENCH`(idle|scroll|drag) 벤치마크 실행 후 종료.

일반 실행에서는 작업공간을 만들거나 열 수 있다. 예제 학과는 가상 자료를 별도 파일에 저장한다. 기존 `.auler` 파일 경로를 인수로 주면 해당 작업공간의 학기 홈을 연다.

**출력·인계**에서 저장된 확정본을 선택해 A4 PDF 또는 Excel 시간표를 만든다. PDF 미리보기에서 쪽 배치를 확인하고, 저장한 PDF를 기본 PDF 앱에서 열어 인쇄할 수 있다. 인계 JSON은 시간표, 기준자료와 선택한 편성 조건을 담는다.

**학교 제출**에서 JSON 양식으로 열 순서, 요일·교시 코드, 머리글과 UTF-8·CP949 인코딩을 지정해 CSV를 저장할 수 있다. [학교 제출 양식 안내](docs/school-submission.md)

**백업·복원**에서 작업공간 전체를 새 `.auler-backup` 파일에 저장할 수 있다. Windows에선 현재 계정에 파일 접근 권한을 부여한다. 복원할 때는 새 경로에 저장하고 무결성과 주요 건수를 확인한 후 복원본을 열지 선택한다.

`auler-cli`에 표준 XLSX 또는 공통 입력 문서 JSON 경로를 주면 검증 후 편성 초안과 미배정 사유를 출력한다. 기본 제한시간은 3초이며 `--time-limit-ms`로 바꿀 수 있다. `--json`은 자동화에서 읽을 수 있는 형식을 사용한다. 종료 코드는 성공 0, 입력·인자 오류 2, 전체 편성이 불가능함을 확인한 경우 3, 제한시간 안에 전체 시간표를 찾지 못한 경우 4다.

## English

Auler is a lightweight, local-first Windows 11 app that helps university department office assistants draft a semester timetable from room data and scheduling constraints, review it, and export it in several formats. AI assistance (built-in ChatGPT or external agents via MCP) is optional: it proposes, a person approves, and the local scheduling engine validates. The UI is native (Rust + Slint) with no browser engine, to keep memory use low.

Development builds support local workspaces, standard XLSX import, manual and automatic scheduling, review, and finalization. Choose a saved final version in **Export & Handover** to create a bilingual A4 PDF, an Excel timetable, or a JSON handover file.

Use **School submission** to export a CSV with your school's column order, day and period codes, headers, and encoding. [School submission profiles](docs/school-submission.md)

Use **Backup & Restore** to save a full workspace copy as a new `.auler-backup` file. On Windows the file grants access to the current account. Restore saves to a new path and checks database integrity and counts before you choose whether to open the copy.

## 만든 사람·라이선스 / Author and license

- 제작자 / Author: [Lehwon](https://github.com/Lehwon)
- 기술 스택 / Tech stack: Rust, [Slint](https://slint.dev) (소프트웨어 렌더러), Win32 API ([windows-sys](https://github.com/microsoft/windows-rs))
- 라이선스 / License: [MIT](LICENSE)
- 사용한 오픈소스와 라이선스 / Third-party components: [THIRD-PARTY.md](THIRD-PARTY.md). Slint는 Royalty-free License 2.0으로 사용하며 앱의 정보 화면에 표시한다.

앱의 **정보** 화면에서도 같은 내용을 볼 수 있다. / The same information is shown on the app's **About** page.
