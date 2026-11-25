use std::path::Path;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .ok_or("Missing output directory")?;
    let output = Path::new(&output);
    std::fs::create_dir_all(output)?;
    println!("ready");
    std::thread::sleep(Duration::from_secs(1));
    let started = Instant::now();
    write_sample(output)?;
    println!("done in {} ms", started.elapsed().as_millis());
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

#[cfg(not(feature = "pdf"))]
fn write_sample(_: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("baseline (PDF renderer disabled)");
    Ok(())
}

#[cfg(feature = "pdf")]
fn write_sample(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use auler_io::pdf::{Language, Preview, Schedule, Section, render, windows_fonts};
    use std::path::PathBuf;

    let mut fonts: Vec<PathBuf> = std::env::args_os().skip(2).map(Into::into).collect();
    if fonts.is_empty() {
        fonts.extend(windows_fonts().ok_or("Supply paths to the Malgun Gothic fonts")?);
    }
    let sections = [Section {
        title: "컴퓨터공학과 2학년".into(),
        rows: (1..=68)
            .map(|number| {
                vec![
                    "월 09:00–10:30".into(),
                    format!("데이터기반지역문화콘텐츠기획및디지털서비스디자인실습 {number:02}"),
                    "김하늘".into(),
                    "공학관 203호".into(),
                ]
            })
            .collect(),
    }];
    let ko = Schedule {
        language: Language::Ko,
        title: "2026학년도 2학기 시간표",
        version: "컴퓨터공학과 · 확정본 v2",
        landscape: false,
        grid: false,
        headings: &["요일 및 시간", "과목 및 분반", "교원", "강의실"].map(String::from),
        column_weights: &[1.3, 2.5, 1.0, 1.2],
        sections: &sections,
    };
    let preview = Some(Preview {
        index: 0,
        width: None,
    });
    let result = render(&ko, &fonts, preview)?;
    let pages = result.pages;
    std::fs::write(output.join("ko.pdf"), result.pdf)?;
    std::fs::write(output.join("ko-preview.png"), result.preview.unwrap())?;
    println!("Korean sample: {pages} A4 pages");
    let sections = [Section {
        title: "Computer Science · Year 2".into(),
        rows: vec![vec![
            "Mon 09:00–10:30".into(),
            "Data-driven Cultural Content and Digital Service Design Lab".into(),
            "Haneul Kim".into(),
            "Engineering 203".into(),
        ]],
    }];
    let en = Schedule {
        language: Language::En,
        title: "Fall 2026 timetable",
        version: "Computer Science · Final version 2",
        landscape: false,
        grid: false,
        headings: &["Day and time", "Course and section", "Instructor", "Room"].map(String::from),
        column_weights: &[1.3, 2.5, 1.0, 1.2],
        sections: &sections,
    };
    let result = render(&en, &fonts, preview)?;
    std::fs::write(output.join("en.pdf"), result.pdf)?;
    std::fs::write(output.join("en-preview.png"), result.preview.unwrap())?;
    Ok(())
}
