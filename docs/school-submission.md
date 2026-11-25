# 학교 제출 CSV / School submission CSV

**출력·인계 → 학교 제출**에서 저장된 확정 버전과 양식 JSON을 선택한다. 미리보기의 열 순서와 인코딩을 확인한 뒤 CSV를 저장한다. 학교에서 받은 규격에 맞춰 **예시 JSON 저장**으로 가상 학과 양식의 복사본을 만든 뒤 수정할 수 있다. [예시 JSON](../templates/school-submission-example.json)

Select a saved final version and a JSON profile under **Export & Handover → School submission**. Check the columns and encoding in the preview, then save the CSV. Use **Save example JSON** to make an editable copy for your school. [Example profile](../templates/school-submission-example.json)

| 설정 / Setting | 값 / Value |
|---|---|
| `format_version` | `1` |
| `name` | 화면에 표시할 이름 / Display name |
| `encoding` | `"utf-8"` 또는 `"cp949"` / `"utf-8"` or `"cp949"` |
| `include_headers` | `true`면 첫 행에 머리글 기록 / Write headers in the first row when `true` |
| `day_codes` | 요일 `mon`~`sun`에 대해 학교에서 정한 문자열 / School values for weekdays `mon` through `sun` |
| `period_codes` | 실제 교시표 순서의 `"1"`, `"2"` 등에 대응하는 문자열 / School values keyed by the period's one-based order in the timetable |
| `columns` | `{ "field": "course_code", "header": "학수번호" }` 같은 항목의 배열. 배열 순서를 사용한다. / An ordered array of field and header pairs. |

| `field` | 의미 / Meaning |
|---|---|
| `course_code`, `course_name`, `section_code` | 학수번호, 과목명, 분반 코드 그대로 / Saved course code, course name, and section code |
| `meeting_number` | 분할 수업 번호. 첫 수업부터 `1` / One-based meeting number within a section |
| `day_code`, `first_period_code`, `last_period_code` | 양식에 지정한 요일과 양 끝 교시 코드 / Configured day and inclusive first and last period codes |
| `start_time`, `end_time` | 실제 시작·종료 시각 `HH:MM` / Saved start and end times as `HH:MM` |
| `building`, `room_number`, `room` | 건물, 호실, 건물과 호실 표기 / Building, room number, or both |
| `instructor_names` | 배치에 해당하는 교원 이름 / Names of the instructors for the class |

CSV에는 확정본의 배치 단위마다 한 행이 들어간다. 값의 선행 0, 쉼표 및 따옴표를 문자열로 보존하고 줄 끝은 CRLF로 저장한다. 선택한 열에 필요한 요일·교시 코드가 빠졌거나 CP949로 표현할 수 없는 문자가 있으면 저장 전에 알려준다.

The CSV has one row per scheduled meeting. It preserves leading zeros and escapes commas and quotes, with CRLF line endings. Missing codes used by the selected columns and characters unavailable in CP949 are reported before saving.

작업공간에서 선택한 양식은 열린 세션에 적용된다. 다른 프로그램에서 제출 파일을 검토할 때는 미리보기나 Excel 출력으로 값과 선행 0을 확인할 수 있다.

The selected profile applies to the open workspace session. Use the preview or Excel export to inspect the values and leading zeros in a spreadsheet workflow.
