# 공통 입력 규격 v1 (`auler.input.v1`)

Auler에 자료가 들어오는 세 경로, 즉 표준 XLSX 가져오기, 내장 ChatGPT 변환, 외부 에이전트(MCP)는 모두 이 형식의 JSON 문서를 만든다. 문서는 같은 검사를 거쳐 검토 초안이 되고, 조교가 확인한 항목만 반영된다. 어느 경로도 시간표를 직접 확정하거나 확정본을 바꿀 수 없다.

코드: [`crates/auler-domain/src/input.rs`](../crates/auler-domain/src/input.rs)(형식), [`resolve.rs`](../crates/auler-domain/src/resolve.rs)(검사). 예: [`samples/reference.json`](../samples/reference.json).

## 원칙

- **코드는 문자열이다.** 학수번호, 분반, 교원·수강 집단·장비 코드, 호실은 문자열로 쓴다. 분반 `01`과 `1`은 다른 분반이다. 숫자로 온 코드는 받되 앞자리 0이 사라졌을 수 있어 "확인 필요"로 표시한다.
- **추측하지 않는다.** 건물 없이 `201호`만 있으면 후보가 하나여도 잇지 않는다. 이름만 있는 교원은 같은 이름이 하나일 때만 잇는다. 변환하는 쪽이 풀지 못한 행은 `unresolved`에 원문과 이유를 남긴다.
- **기록은 자연 키로 서로를 가리킨다.** 분반은 학수번호+분반 코드, 강의실은 건물+호실, 교원은 교원 코드(없으면 이름).
- **값은 느슨하게 받고 검사에서 알린다.** 요일·시각·날짜·종류는 문자열로 받아, 잘못된 값을 문서 안 경로(`sections[3].kind`)와 원본 위치와 함께 한꺼번에 알린다. 모르는 필드가 있으면 문서를 읽지 않는다.
- **원본 위치를 남긴다.** 모든 기록에 `source`(파일, 시트, 행, 열)를 붙일 수 있다. 검토 화면은 원본과 변환값을 나란히 보여준다.
- **조건의 `status`는 해석 상태다.** `confirmed`는 대상·시간·강도가 정해졌다는 뜻이고, `pending`은 사람의 확인이 필요하다는 뜻이다. 어느 쪽이든 검토함에서 승인해야 적용되며, `pending` 조건은 승인 뒤에도 편성에 쓰지 않는다.

## 문서

```json
{
  "schema": "auler.input.v1",
  "term": { "year": 2027, "season": "1" },
  "setup": { ... },
  "equipment": [ ... ],
  "rooms": [ ... ],
  "instructors": [ ... ],
  "groups": [ ... ],
  "courses": [ ... ],
  "sections": [ ... ],
  "constraints": [ ... ],
  "placements": [ ... ],
  "unresolved": [ ... ]
}
```

`schema`와 `term`만 반드시 있어야 한다. 나머지는 필요한 것만 쓴다. `season`은 `1`, `summer`, `2`, `winter`.

### setup: 학기 설정

| 필드 | 형식 | 설명 |
|---|---|---|
| `start`, `end` | `YYYY-MM-DD` | 학기 기간 |
| `days` | 요일 목록 | `mon`…`sun` (`월`, `Monday`도 읽는다) |
| `periods` | `{label, start, end}` 목록 | 교시. 시각은 `HH:MM`, 시각 순서로 겹치지 않아야 한다. 교시 길이가 일정할 필요는 없다 |
| `breaks` | `{label, start, end}` 목록 | 이름 붙은 휴게시간(점심 등). 교시 사이의 빈 시간 안에 있어야 한다 |
| `rules.required_course_overlap` | `hard` \| `soft` | 같은 수강 집단의 필수과목끼리 겹칠 때의 강도. 기본 `hard` |
| `rules.block_may_cross_break` | bool | 연강이 휴게시간을 넘어갈 수 있는지. 기본 `false` |
| `rules.room_turnaround_minutes` | 정수 | 같은 강의실의 앞 수업 종료와 다음 수업 시작 사이에 필요한 준비·정리 시간(분). 기본 0 |

### equipment, rooms: 장비와 강의실

장비 `{code, name, scope}`. `scope`가 `per_person`이면 수강생 한 명마다(실습용 PC), `per_room`이면 강의실에 있으면 된다(빔프로젝터).

강의실 `{building, number, kind, department, total_seats, available_seats, equipment, shared, schedule_checked, info_source}`. `number` 끝의 `호`는 떼고 저장한다. `kind`는 `lecture`, `lab`, `seminar`, `other`. 장비는 `{code, owned, available, inspection_from, inspection_until}`이며 `available`은 고장 등을 뺀 정상 수량이다. `shared`인 공용실은 `schedule_checked`(외부 일정 확인일)가 없으면 빈 방으로 보지 않는다.

### instructors, groups, courses: 교원, 수강 집단, 과목

교원 `{code, name, department}`. 이름이 같은 교원이 있을 수 있어 코드로 구분한다. 수강 집단 `{code, name, grade, track}`은 같은 시간에 수업을 들어야 하는 학생 묶음(학년, 반, 트랙)이다. 과목 `{code, name}`.

### sections: 분반

| 필드 | 설명 |
|---|---|
| `course`, `section` | 학수번호, 분반 코드 |
| `instructors` | `{code}` 또는 `{name}` 목록. 비어 있으면 미정 |
| `groups` | `{group, required}` 목록. `required`면 그 집단의 필수과목 |
| `enrollment`, `provisional` | 인원, 잠정 여부 |
| `kind`, `room_kind` | 수업 형태(`lecture`, `lab`, `mixed`), 필요한 강의실 유형 |
| `weekly_periods`, `split` | 주당 시수와 배치 단위별 연속 교시 수. 주당 3시간을 2+1로 나누면 `[2, 1]`. 없으면 한 번에 배치 |
| `equipment` | `{code, quantity}` 목록. 1인당 장비면 한 명당 수량 |

### constraints: 편성 조건

| 필드 | 설명 |
|---|---|
| `key` | 문서 안에서 조건을 구분하는 키 (선택) |
| `target` | `instructor`, `section`(`{course, section}`), `group`, `room` 중 하나만 |
| `rule` | `{kind: "avoid_times", windows: [...]}`. 해석하지 못한 요청은 비운다 |
| `strength`, `weight` | `hard`(어기면 확정 불가) \| `soft`(경고), 선호 조건의 상대 중요도 1~10 |
| `status` | `confirmed` \| `pending` |
| `from`, `until` | 적용 기간. 없으면 학기 전체. 한쪽만 있으면 나머지는 학기 시작·종료일 |
| `text` | 요청 원문 |
| `confirmed_by`, `note` | 확인자, 내부 메모. 내부 메모는 출력과 인계에 기본으로 넣지 않는다 |

시간 범위 `window`는 교시(`{day, first_period, last_period}`, 양 끝 포함)나 시각(`{day, start, end}`, 시작 포함·종료 제외) 중 하나로 적는다. 예: "수요일 13~16시는 가능하면 비워 주세요"는 `{day: "wed", start: "13:00", end: "16:00"}`, `soft`. "목요일 1교시 수업 불가"는 `{day: "thu", first_period: "1"}`, `hard`. "오전 수업 어려움"은 `rule` 없이 `pending`.

### placements: 배치

`{course, section, part, day, start_period, room, locked}`. `part`는 분반의 몇 번째 배치 단위인지(1부터), `start_period`는 교시 이름. 강의실 참조는 `{building, number}` 또는 원본 표기 `{text: "공학관 302호"}`.

### unresolved: 변환하지 못한 행

`{what, text, reason, source}`. 예: 건물이 없는 강의실 행, 편성 요청이 아닌 지시문. 모두 "확인 필요"로 검토 화면에 나온다.

## 검사 결과

검사는 문제마다 문서 안 경로, 종류, 원본 위치를 돌려준다. 종류는 고정 이름(`code()`)이 있어 화면 문구로 번역한다.

| 무게 | 종류 |
|---|---|
| 확인 필요 | `numeric_code`(숫자로 온 코드), `ambiguous_room`(건물 없는 호실 등), `ambiguous_instructor`(동명이인), `instructor_name_mismatch`(코드와 이름 불일치), `unresolved` |
| 오류 | `unsupported_schema`, `missing`, `invalid_code`, `invalid_value`, `invalid_room`, `invalid_period_table`, `duplicate`, `unknown_reference`, `split_mismatch`, `invalid_split_part`, `available_exceeds_total`, `reversed_range`, `ambiguous_window`, `target_not_single`, `confirmed_incomplete`, `part_out_of_range`, `span_past_last_period`, `day_not_in_term`, `zero`, `weight_out_of_range` |

## 표준 XLSX 양식 v1

[`templates/auler-standard-v1.ko.xlsx`](../templates/auler-standard-v1.ko.xlsx) (영어판 [`en`](../templates/auler-standard-v1.en.xlsx)). 시트·열 정의는 [`template.rs`](../crates/auler-domain/src/template.rs) 하나를 양식 생성과 가져오기가 함께 쓴다.

- 안내 시트 B2에 양식 버전 `auler.xlsx.v1`이 있어야 한다.
- 시트: 학기, 교시표, 휴게시간, 장비, 강의실, 강의실 장비, 교원, 수강 집단, 과목, 분반, 분반 수강 집단, 분반 장비, 편성 조건, 배치. 휴게시간·장비·강의실 장비·수강 집단·분반 수강 집단·분반 장비·편성 조건·배치는 없어도 된다.
- 첫 행이 머리글이다. 한국어나 영어 머리글 모두 받고 열 순서는 상관없다. 모르는 머리글이 있으면 가져오지 않는다(열을 임의로 잇지 않는다).
- 코드 열은 텍스트 서식이다. 숫자 칸으로 바뀐 코드는 "확인 필요"로 표시한다.
- 여러 값은 `;`로(`월;화;수`), 분할은 `+`로(`2+1`) 적는다. 예/아니오 열과 정해진 값 열에는 목록 선택이 있다.
- 편성 조건 시트는 한 행이 시간 범위 하나다. 조건 번호가 같은 행은 한 조건으로 묶는다.

## 가상 데이터

[`samples/`](../samples)는 `cargo run -p auler-samples`로 만든다. 실제 학교·사람과 관계없다.

- `reference.json`: 기준 학과(컴퓨터공학과 2027년 1학기). 분반 42, 교원 28, 강의실 12. 장비 부족 실습실, 다른 학과 사용 시간, 일정 미확인 공용실, 동명이인, 같은 호실 번호, 확인 대기 요청, 잠근 배치를 담았다.
- `stress.json`: 분반 150. 검증·편성 시간 측정용.
- `messy/`: 학과 사무실에서 흔한 모양의 원본 엑셀 세 개(병합 셀, 두 줄 머리글, 숫자로 저장된 분반, 건물 없는 호실, 한 칸에 섞인 요청, 요청이 아닌 지시문)와 그 올바른 변환 결과 `expected.json`. AI·MCP 변환 시험의 정답이다.
