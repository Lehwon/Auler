mod report;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use auler_domain::input::InputDocument;
use auler_domain::resolve::resolve;
use auler_engine::{LocalSolver, SolveOptions, SolveStatus, Solver};
use auler_io::xlsx::ImportError;

use report::{Diagnostic, ResultReport, write_errors, write_result};

const USAGE: &str =
    "사용법: auler-cli <*.xlsx 또는 *.json> [--time-limit-ms 밀리초] [--seed 숫자] [--json]";

struct Arguments {
    path: PathBuf,
    options: SolveOptions,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Arguments>, &'static str> {
    let mut args = args.into_iter();
    let mut path = None;
    let mut options = SolveOptions::default();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(None),
            Some("--time-limit-ms") => {
                options.time_limit = Duration::from_millis(
                    next_number(&mut args).ok_or("시간 제한에는 0 이상의 정수를 입력하세요.")?,
                );
            }
            Some("--seed") => {
                options.seed =
                    next_number(&mut args).ok_or("시드에는 0 이상의 정수를 입력하세요.")?;
            }
            Some("--json") => {}
            Some(flag) if flag.starts_with('-') => return Err("지원하지 않는 옵션입니다."),
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => return Err("입력 파일은 하나만 지정하세요."),
        }
    }
    let path = path.ok_or("가져올 파일을 지정하세요.")?;
    Ok(Some(Arguments { path, options }))
}

fn next_number(args: &mut impl Iterator<Item = OsString>) -> Option<u64> {
    args.next()?.to_str()?.parse().ok()
}

fn read(path: &Path) -> Result<InputDocument, Vec<Diagnostic>> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("xlsx") => auler_io::xlsx::import(path).map_err(|error| match error {
            ImportError::Validation(issues) => issues.into_iter().map(Diagnostic::xlsx).collect(),
            ImportError::Workbook { source, error } => {
                let mut diag = Diagnostic::file("xlsx_read_error", path, error.to_string());
                diag.source = Some(source);
                vec![diag]
            }
            ImportError::Document(error) => {
                vec![Diagnostic::file("xlsx_document", path, error.to_string())]
            }
        }),
        Some("json") => {
            let text = std::fs::read_to_string(path).map_err(|error| {
                vec![Diagnostic::file("json_read_error", path, error.to_string())]
            })?;
            InputDocument::from_json(text.trim_start_matches('\u{feff}'))
                .map_err(|error| vec![Diagnostic::file("invalid_json", path, error.to_string())])
        }
        _ => Err(vec![Diagnostic::file(
            "unsupported_file",
            path,
            "표준 XLSX 또는 공통 입력 문서 JSON 파일을 지정하세요.".into(),
        )]),
    }
}

fn run(args: Arguments, json: bool) -> io::Result<ExitCode> {
    let data = match read(&args.path).and_then(|doc| {
        resolve(&doc).map_err(|issues| issues.into_iter().map(Diagnostic::input).collect())
    }) {
        Ok(data) => data,
        Err(issues) => {
            return reported_exit(
                ExitCode::from(2),
                write_errors("invalid_input", &issues, json),
            );
        }
    };
    let result = LocalSolver.solve(&data, args.options, &AtomicBool::new(false), &mut |_| {});
    let code = match result.status {
        SolveStatus::Solved => ExitCode::SUCCESS,
        SolveStatus::NoSolution => ExitCode::from(3),
        SolveStatus::TimedOut => ExitCode::from(4),
        SolveStatus::Cancelled => ExitCode::from(130),
    };
    reported_exit(code, write_result(&ResultReport::new(&data, &result), json))
}

fn reported_exit(code: ExitCode, written: io::Result<()>) -> io::Result<ExitCode> {
    match written {
        Ok(()) => Ok(code),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(code),
        Err(error) => Err(error),
    }
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let json = args.iter().any(|arg| arg == "--json");
    let result = match parse(args) {
        Ok(Some(args)) => run(args, json),
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            let issue = Diagnostic::arguments(error, USAGE);
            reported_exit(
                ExitCode::from(2),
                write_errors("invalid_arguments", &[issue], json),
            )
        }
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("출력 오류 ({error})");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_result_pipe_does_not_convert_failure_to_success() {
        for code in [2, 3, 4] {
            let exit = ExitCode::from(code);
            let broken_pipe = io::Error::from(io::ErrorKind::BrokenPipe);
            assert_eq!(reported_exit(exit, Err(broken_pipe)).unwrap(), exit);
        }
    }
}
