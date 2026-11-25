# Windows 설치본 첫 측정

Windows 11 Pro x64 빌드 26300에서 `tools/package-windows.ps1`로 생성한 Auler 0.1.0 사전판을 설치했다. 설치본은 21,541,650바이트(20.54 MiB), 설치된 실행 파일은 74,340,352바이트(70.90 MiB)였다. `dumpbin /DEPENDENTS`에서 Windows 기본 DLL 의존성을 확인했다. C 런타임을 정적으로 연결해 별도의 Visual C++ 재배포 패키지 없이 실행할 수 있게 했다.

프로세스별 `Private Bytes`와 `Working Set`을 `tools/measure-memory.ps1`로 측정했다. 각 경우 언어를 영어로 고정하고 4초 후 300ms 간격으로 다섯 번 측정했다. 작업공간 측정에는 42개 분반, 68개 배치, 확정본 1개가 있는 가상 학과를 사용했다.

| 시점 | 프로세스 | Private Bytes 평균 / 최대 | Working Set 평균 |
|---|---:|---:|---:|
| 시작 화면 | 1 | 20.2 / 20.2 MiB | 54.6 MiB |
| 확정된 가상 학과 홈 | 1 | 21.3 / 21.3 MiB | 43.4 MiB |

현재 사용자 설치에서 시작 메뉴 바로가기와 제거 항목을 확인했다. 설치된 실행 파일, 라이선스 두 파일, 설치 안내, XLSX 두 벌과 JSON 예시의 SHA-256이 빌드 및 저장소 파일과 모두 일치했다. 제거 후 실행 파일, 설치 등록과 바로가기가 없어졌고, 설치 폴더 밖에 둔 백업의 SHA-256은 보존됐다.

한글 PDF에는 Windows 11 기본 글꼴 목록에 있는 맑은 고딕 보통체·굵은체를 사용한다. [Microsoft의 Windows 11 글꼴 목록](https://learn.microsoft.com/en-us/typography/fonts/windows_11_font_list)

이 측정은 개발 도구가 있는 시험용 PC에서 진행했다. 깨끗한 Windows 이미지에서의 오프라인 설치와 한글 PDF 검증, 편집·편성·PDF 출력 중 앱 프로세스 피크는 별도 검증 항목이다. [Typst 단독 측정 기록](2026-11-25-typst-spike.md)
