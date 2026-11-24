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
cargo build --release -p auler     # 릴리스 빌드
./tools/measure-memory.ps1 -Exe target/release/auler.exe -Label idle   # 메모리 측정
```

## English

Auler is a lightweight, local-first Windows 11 app that helps university department office assistants draft a semester timetable from room data and scheduling constraints, review it, and export it in several formats. AI assistance (built-in ChatGPT or external agents via MCP) is optional: it proposes, a person approves, and the local scheduling engine validates. The UI is native (Rust + Slint) with no browser engine, to keep memory use low.

This project is in early development and not usable yet.

## License

[MIT](LICENSE)
