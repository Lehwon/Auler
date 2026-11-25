//! Offline PDF and page previews from the same trusted Typst layout.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::{LazyHash, Scalar};
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;
use typst_render::RenderOptions;

static COMPILE: Mutex<()> = Mutex::new(());
const TEMPLATE: &str = include_str!("schedule.typ");

pub use crate::export::Language;
use crate::export::{Fields, Layout, Snapshot, View};

#[derive(Serialize)]
pub struct Schedule<'a> {
    pub language: Language,
    pub title: &'a str,
    pub version: &'a str,
    pub landscape: bool,
    pub grid: bool,
    pub headings: &'a [String],
    pub column_weights: &'a [f32],
    pub sections: &'a [Section],
}

#[derive(Serialize)]
pub struct Section {
    pub title: String,
    pub rows: Vec<Vec<String>>,
}

pub struct Output {
    pub pdf: Vec<u8>,
    pub pages: usize,
    pub preview: Option<Vec<u8>>,
}

#[derive(Clone, Copy)]
pub struct Preview {
    pub index: usize,
    pub width: Option<u32>,
}

pub struct TimetableOptions {
    pub locale: Language,
    pub target: View,
    pub layout: Layout,
    pub fields: Fields,
    pub landscape: bool,
}

pub fn timetable(
    snapshot: &Snapshot,
    options: &TimetableOptions,
    preview: Option<Preview>,
) -> Result<Output, String> {
    let TimetableOptions {
        locale,
        target,
        layout,
        fields,
        landscape,
    } = *options;
    let grid = matches!(layout, Layout::Grid);
    let table = if grid {
        snapshot.print_grid(target, locale, fields)
    } else {
        snapshot.table(target, locale, layout, fields)
    };
    let title = format!(
        "{} {}",
        locale.term(snapshot.info().term),
        locale.choose("시간표", "timetable")
    );
    let version = format!(
        "{} · {}",
        snapshot.department(),
        snapshot.version_label(locale)
    );
    let weights = if grid {
        vec![1.0; table.headings.len()]
    } else {
        let mut weights = vec![0.65, 0.75, 0.75, 1.0, 0.85];
        if fields.course_name {
            weights.push(2.5);
        }
        if fields.instructors {
            weights.push(1.45);
        }
        if fields.room {
            weights.push(1.55);
        }
        weights
    };
    let schedule = Schedule {
        language: locale,
        title: &title,
        version: &version,
        landscape,
        grid,
        headings: &table.headings,
        column_weights: &weights,
        sections: &[Section {
            title: snapshot.view_label(target, locale),
            rows: table.rows,
        }],
    };
    let fonts = windows_fonts().ok_or("Missing Windows fonts folder")?;
    render(&schedule, &fonts, preview)
}

/// Locate the Windows fonts without distributing them with the application.
pub fn windows_fonts() -> Option<[PathBuf; 2]> {
    let directory = PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts");
    Some([directory.join("malgun.ttf"), directory.join("malgunbd.ttf")])
}

/// The optional preview page is zero based. Only its PNG and the PDF leave the renderer.
pub fn render(
    schedule: &Schedule<'_>,
    font_paths: &[impl AsRef<Path>],
    preview_page: Option<Preview>,
) -> Result<Output, String> {
    let _lock = COMPILE.lock().map_err(|_| "PDF renderer unavailable")?;
    let _clear_cache = ClearCache;
    let data = serde_json::to_vec(schedule).map_err(|error| error.to_string())?;
    let world = PrintWorld::new(data, font_paths)?;
    let compiled = typst::compile::<PagedDocument>(&world);
    let document = compiled.output.map_err(diagnostics)?;
    if !compiled.warnings.is_empty() {
        return Err(diagnostics(compiled.warnings));
    }
    let preview = preview_page
        .map(|preview| {
            let page = document
                .pages()
                .get(preview.index)
                .ok_or("Invalid preview page")?;
            let options = RenderOptions {
                pixel_per_pt: preview.width.map_or(
                    RenderOptions::default().pixel_per_pt,
                    |width| {
                        Scalar::new(f64::from(width.clamp(100, 2400)) / page.frame.width().to_pt())
                    },
                ),
                ..RenderOptions::default()
            };
            typst_render::render(page, &options)
                .encode_png()
                .map_err(|error| error.to_string())
        })
        .transpose()?;
    let pdf = typst_pdf::pdf(&document, &PdfOptions::default()).map_err(diagnostics)?;
    Ok(Output {
        pdf,
        pages: document.pages().len(),
        preview,
    })
}

fn diagnostics(warnings: typst::ecow::EcoVec<typst::diag::SourceDiagnostic>) -> String {
    warnings
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

struct ClearCache;

impl Drop for ClearCache {
    fn drop(&mut self) {
        typst::comemo::evict(0);
    }
}

struct PrintWorld {
    library: LazyHash<Library>,
    source: Source,
    data_id: FileId,
    data: Bytes,
    fonts: Vec<Font>,
    book: LazyHash<FontBook>,
}

impl PrintWorld {
    fn new(data: Vec<u8>, font_paths: &[impl AsRef<Path>]) -> Result<Self, String> {
        let mut fonts = Vec::new();
        for path in font_paths {
            let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
            fonts.extend(Font::iter(Bytes::new(bytes)));
        }
        if fonts.is_empty() {
            return Err("The PDF font could not be read".into());
        }
        let book = LazyHash::new(FontBook::from_fonts(&fonts));
        Ok(Self {
            library: LazyHash::new(Library::default()),
            source: Source::new(template_file("schedule.typ"), TEMPLATE.to_owned()),
            data_id: template_file("schedule.json"),
            data: Bytes::new(data),
            fonts,
            book,
        })
    }
}

fn template_file(name: &str) -> FileId {
    RootedPath::new(VirtualRoot::Project, VirtualPath::new(name).unwrap()).intern()
}

impl World for PrintWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main() {
            Ok(self.source.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.data_id {
            Ok(self.data.clone())
        } else if id == self.main() {
            Ok(Bytes::from_string(TEMPLATE))
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}
