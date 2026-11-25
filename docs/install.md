# Auler 설치 / Install Auler

## 한국어

Windows 11 x64용 설치본입니다. 현재 Windows 계정에 설치되며 관리자 권한이 필요 없습니다. 설치 파일을 받아 두면 설치와 로컬 시간표 작업을 오프라인에서 진행할 수 있습니다.

1. GitHub Releases에서 `auler_*_x64-setup.exe` 설치본과 함께 제공된 `.sha256` 파일을 받습니다. 사전 시험에는 담당자가 제공한 설치본을 사용합니다. 두 파일을 받은 폴더에서 PowerShell을 열고 `Get-FileHash .\auler_*_x64-setup.exe -Algorithm SHA256`을 실행합니다. `.sha256`에 적힌 파일 이름과 SHA-256 값을 비교합니다. 영문 대소문자는 무시합니다. 이 사전판 설치본은 미서명 상태라 Windows에서 게시자 확인 메시지가 표시될 수 있습니다. [GitHub Releases](https://github.com/Lehwon/Auler/releases)
2. 설치 파일을 열고 한국어 또는 영어를 선택합니다. 기본 설치 폴더를 그대로 쓰거나 다른 폴더를 선택하고 설치를 완료합니다.
3. 시작 메뉴에서 **Auler**를 엽니다. **예제 열기**로 가상 자료를 살펴보거나 **새 작업공간**에서 **만들기**를 선택합니다. 작업공간은 `문서` 폴더 등 원하는 위치에 저장합니다.

표준 XLSX 양식과 학교 제출 양식 예시는 설치 폴더의 `templates`에 있습니다. 기본 경로는 `%LOCALAPPDATA%\Auler\templates`입니다. 작업공간을 연 뒤 **기준자료 → 가져오기**에서 작성한 표준 XLSX를 엽니다.

앱을 제거하려면 Windows **설정 → 앱 → 설치된 앱 → Auler → 제거**를 선택합니다. 저장한 `.auler` 작업공간과 `.auler-backup` 백업 파일은 남아 있으므로 새 설치에서도 열 수 있습니다.

## English

The installer targets Windows 11 x64. It installs for the current Windows account without administrator privileges. After downloading the installer, you can install the app and work on local timetables offline.

1. Download the `auler_*_x64-setup.exe` installer and its `.sha256` file from GitHub Releases when available. For pre-release testing, use the installer supplied by the project maintainer. Open PowerShell in the download folder and run `Get-FileHash .\auler_*_x64-setup.exe -Algorithm SHA256`. Compare the file name and hash with the `.sha256` file. Ignore differences in letter case. Windows may display a publisher prompt because this pre-release installer is unsigned. [GitHub Releases](https://github.com/Lehwon/Auler/releases)
2. Open the installer and select English or Korean. Keep the default installation folder or choose a different one, then finish installing.
3. Open **Auler** from the Start menu. Choose **Open sample** to explore fictional data or **Create** under **New workspace** to start your timetable. Save the workspace in a folder you choose, such as Documents.

The installed `templates` folder contains the standard XLSX templates and an example school submission profile. The default location is `%LOCALAPPDATA%\Auler\templates`. Open a workspace and choose **Data → Import** to open a filled standard workbook.

To uninstall, choose **Settings → Apps → Installed apps → Auler → Uninstall**. Your saved `.auler` workspaces and `.auler-backup` copies remain on disk and can be opened from a new installation.

## 설치본 빌드 / Build the installer

Windows 개발 환경에서 저장소 루트로 이동해 실행합니다. / Run from the repository root on a Windows development machine.

```powershell
cargo install cargo-packager --version 0.11.8 --locked --root target/tools/cargo-packager
./tools/package-windows.ps1
```

설치본과 SHA-256 파일은 `target/windows-installer/`에 생성됩니다. 첫 패키징 시 빌드용 NSIS 도구를 내려받습니다. / The installer and its SHA-256 file are written to `target/windows-installer/`. The first packaging run downloads the NSIS build tools.
