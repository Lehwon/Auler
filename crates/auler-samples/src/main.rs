//! `cargo run -p auler-samples`: 저장소의 `samples/`와 `templates/`를 다시 만든다.

use std::process::ExitCode;

fn main() -> ExitCode {
    let root = auler_samples::repo_root();
    let outputs = match auler_samples::outputs() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("생성 실패: {e}");
            return ExitCode::FAILURE;
        }
    };
    for (path, bytes) in outputs {
        let full = root.join(&path);
        if let Some(dir) = full.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            eprintln!("{}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
        if let Err(e) = std::fs::write(&full, &bytes) {
            eprintln!("{}: {e}", full.display());
            return ExitCode::FAILURE;
        }
        println!("{} ({} bytes)", path.display(), bytes.len());
    }
    ExitCode::SUCCESS
}
