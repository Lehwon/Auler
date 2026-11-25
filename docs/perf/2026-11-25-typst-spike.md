# Windows 내장 PDF 출력 측정

2026-11-25, Windows 11 Pro 26300, Intel Core Ultra 7 255H, RAM 23.5 GiB, Rust 1.100.0 MSVC. 릴리스 프로필은 Thin LTO, 단일 코드 생성 단위, 심볼 제거를 사용한다. [Typst 0.15.1](https://github.com/typst/typst/releases/tag/v0.15.1)을 Rust 라이브러리로 연결했다. Windows 11에 설치된 맑은 고딕 보통체와 굵은체를 열고 사용한 글리프만 PDF에 넣는다. [Windows 11 글꼴 목록](https://learn.microsoft.com/en-us/typography/fonts/windows_11_font_list)

```powershell
./tools/measure-pdf-spike.ps1 | Format-List
```

동일한 측정용 실행 파일을 `pdf` 기능을 끄고 켜서 비교했다. 메모리 수집 루프는 매회 10 ms를 쉬며 실제 표본 간격은 약 30 ms였다. 순간 피크는 Windows가 기록한 최대 커밋을 함께 확인했다. 요약 JSON과 표본 CSV는 `target/tmp/pdf-spike/`에 저장된다. Typst 사용 시 합성 데이터 한글 68행과 영문 1행을 연속으로 각각 PDF와 첫 페이지 PNG로 기록했다. 출력 전과 후에는 같은 프로세스를 유지했다.

| 측정 | PDF 기능 끔 | PDF 기능 켬 |
|---|---:|---:|
| 실행 파일 크기 | 151,552 B | 39,154,688 B |
| 출력 전 Private Bytes | 0.57 MiB | 0.82 MiB |
| 표본 중 최대 Private Bytes | 0.57 MiB | 49.39 MiB |
| Windows 기록 최대 커밋 | 0.58 MiB | 50.02 MiB |
| 출력 후 Private Bytes | 0.57 MiB | 5.41 MiB |
| 두 언어 출력 및 파일 기록 | — | 117 ms |

측정용 실행 파일에서 PDF 기능은 37.20 MiB를 추가했다. 렌더러는 요청 때 문서와 글꼴을 만들고 반환 전에 Typst 캐시를 비운다. 출력 후 Private Bytes는 진입 시점보다 4.59 MiB 높았다. 앱의 최종 설치본 크기와 연속 출력 후 메모리는 데스크톱 흐름에서 다시 잰다.

PyMuPDF로 저장 파일을 다시 열어 한글 4쪽과 영문 1쪽이 각각 A4이고, 모든 쪽에 열 제목, 학기·버전·관점, 현재 쪽과 전체 쪽 수, 내장 글꼴이 있음을 확인했다. 한글 긴 과목명이 열 안에서 줄바꿈되고 분반 `01`부터 `68`까지 텍스트로 추출됐다. 같은 Typst 결과에서 만든 1191×1684 PNG와 저장된 PDF의 첫 쪽을 눈으로 대조했다.
