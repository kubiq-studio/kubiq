//! The `.ipynb` file: nbformat 4.x read into cells, and written back the way it was written.
//!
//! **Nothing the model does not understand is lost.** Every cell and the top-level object keep the
//! parsed [`Json`] they came from; [`NotebookFile::serialize`] starts from it and overwrites only
//! the fields the model owns (`cell_type`, `id`, `source`, `outputs`, `execution_count`,
//! `metadata`), each in place. A field whose value did not change keeps its original form — a
//! `source` written as one string stays one string, a number keeps its digits — so an unedited
//! notebook re-serialises byte for byte, whichever tool wrote it (JupyterLab's sorted one-space
//! style, VS Code's, Colab's unsorted two-space one; [`Style`] is read off the file).
//!
//! **Ids.** nbformat 4.5 gives every cell an `id`; older files have none. One is minted in memory
//! on read, and written — with `nbformat_minor` raised to 5 — only once the document has been
//! edited, so opening and saving an old notebook untouched changes nothing.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::json::{self, Json, Style};

/// What a cell is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellKind {
    Code,
    Markdown,
    Raw,
}

impl CellKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CellKind::Code => "code",
            CellKind::Markdown => "markdown",
            CellKind::Raw => "raw",
        }
    }

    fn of(s: &str) -> CellKind {
        match s {
            "code" => CellKind::Code,
            "markdown" => CellKind::Markdown,
            _ => CellKind::Raw,
        }
    }
}

/// One output, as the interface draws it and an agent reads it. The saved form stays the raw
/// nbformat object ([`StoredOutput::raw`]); this is derived from it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Output {
    /// `stdout` / `stderr` text, unprocessed (`\r` and ANSI are the renderer's: [`crate::output`]).
    Stream { name: String, text: String },
    /// `display_data` or `execute_result`: a MIME bundle in the file's order. A JSON MIME type's
    /// value is its compact JSON text; every other value is the joined string (base64 for images).
    Data {
        execute_result: bool,
        execution_count: Option<i64>,
        bundle: Vec<(String, String)>,
    },
    Error {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
}

/// An output as the document holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredOutput {
    /// The nbformat object, exactly as read or as built from the kernel's message.
    pub raw: Json,
    /// `transient.display_id`: never saved, used by `update_display_data`.
    pub display_id: Option<String>,
}

impl StoredOutput {
    pub fn new(raw: Json) -> Self {
        StoredOutput {
            raw,
            display_id: None,
        }
    }

    pub fn view(&self) -> Output {
        output_view(&self.raw)
    }
}

/// One cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub id: String,
    pub kind: CellKind,
    pub source: String,
    pub execution_count: Option<i64>,
    pub outputs: Vec<StoredOutput>,
    pub metadata: Json,
    /// The object it was read from (`{}` for a cell Kubiq created); unknown keys live here.
    raw: Json,
    /// The id was minted on read, not found in the file.
    id_minted: bool,
}

impl Cell {
    /// A fresh cell, with the keys Jupyter gives one.
    pub fn new(id: String, kind: CellKind, source: String) -> Cell {
        Cell {
            id,
            kind,
            source,
            execution_count: None,
            outputs: Vec::new(),
            metadata: Json::Obj(Vec::new()),
            raw: Json::Obj(Vec::new()),
            id_minted: false,
        }
    }

    fn serialize(&self, write_id: bool) -> Json {
        let mut obj = self.raw.clone();
        obj.set("cell_type", Json::str(self.kind.as_str()));
        if !self.id_minted || write_id {
            obj.set("id", Json::str(&self.id));
        }
        obj.set("metadata", self.metadata.clone());
        let source_kept = self.raw.get("source").and_then(Json::as_multiline).as_deref()
            == Some(self.source.as_str());
        if !source_kept {
            let as_string = matches!(self.raw.get("source"), Some(Json::Str(_)));
            obj.set(
                "source",
                if as_string {
                    Json::str(&self.source)
                } else {
                    Json::lines(&self.source)
                },
            );
        }
        if self.kind == CellKind::Code {
            let count_kept = match self.raw.get("execution_count") {
                Some(Json::Null) => self.execution_count.is_none(),
                Some(n) => n.as_i64().is_some() && n.as_i64() == self.execution_count,
                None => false,
            };
            if !count_kept {
                obj.set(
                    "execution_count",
                    self.execution_count
                        .map_or(Json::Null, |n| Json::Num(n.to_string())),
                );
            }
            obj.set(
                "outputs",
                Json::Arr(self.outputs.iter().map(|o| o.raw.clone()).collect()),
            );
        } else {
            obj.remove("execution_count");
            obj.remove("outputs");
        }
        obj
    }
}

/// A read notebook.
#[derive(Debug, Clone, PartialEq)]
pub struct NotebookFile {
    pub cells: Vec<Cell>,
    pub metadata: Json,
    pub nbformat_minor: i64,
    root: Json,
    style: Style,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError(pub String);

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FormatError {}

/// An id the way JupyterLab writes them: 8 hex digits — here derived from a seed the host draws
/// and a counter, so the leaf needs no randomness.
pub fn mint_id(seed: u64, n: u64) -> String {
    let mut h = Sha256::new();
    h.update(seed.to_le_bytes());
    h.update(n.to_le_bytes());
    h.finalize()[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl NotebookFile {
    /// An empty nbformat 4.5 notebook with one empty code cell.
    pub fn empty(first_id: String) -> NotebookFile {
        NotebookFile {
            cells: vec![Cell::new(first_id, CellKind::Code, String::new())],
            metadata: Json::Obj(Vec::new()),
            nbformat_minor: 5,
            root: Json::sorted(vec![
                ("nbformat".into(), Json::Num("4".into())),
                ("nbformat_minor".into(), Json::Num("5".into())),
            ]),
            style: Style::default(),
        }
    }

    /// Read a notebook. `mint` names a cell that has no id.
    pub fn parse(text: &str, mint: &mut dyn FnMut() -> String) -> Result<NotebookFile, FormatError> {
        let root = json::parse(text).map_err(|e| FormatError(format!("not JSON: {e}")))?;
        if !matches!(root, Json::Obj(_)) {
            return Err(FormatError("not a notebook: the top level is not an object".into()));
        }
        let major = root.get("nbformat").and_then(Json::as_i64);
        if major != Some(4) {
            return Err(FormatError(format!(
                "unsupported nbformat {}: only 4.x is read",
                major.map_or("?".into(), |m| m.to_string())
            )));
        }
        let nbformat_minor = root.get("nbformat_minor").and_then(Json::as_i64).unwrap_or(0);
        let metadata = root.get("metadata").cloned().unwrap_or(Json::Obj(Vec::new()));
        let mut cells = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for raw in root.get("cells").and_then(Json::as_array).unwrap_or(&[]) {
            let kind = CellKind::of(raw.get("cell_type").and_then(Json::as_str).unwrap_or("raw"));
            let found = raw.get("id").and_then(Json::as_str).map(str::to_string);
            let (id, id_minted) = match found {
                Some(id) if seen.insert(id.clone()) => (id, false),
                _ => loop {
                    let id = mint();
                    if seen.insert(id.clone()) {
                        break (id, true);
                    }
                },
            };
            cells.push(Cell {
                id,
                kind,
                source: raw.get("source").and_then(Json::as_multiline).unwrap_or_default(),
                execution_count: raw.get("execution_count").and_then(Json::as_i64),
                outputs: raw
                    .get("outputs")
                    .and_then(Json::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .cloned()
                    .map(StoredOutput::new)
                    .collect(),
                metadata: raw.get("metadata").cloned().unwrap_or(Json::Obj(Vec::new())),
                raw: raw.clone(),
                id_minted,
            });
        }
        Ok(NotebookFile {
            cells,
            metadata,
            nbformat_minor,
            root,
            style: Style::detect(text),
        })
    }

    /// Some cell's id was minted on read.
    pub fn minted_ids(&self) -> bool {
        self.cells.iter().any(|c| c.id_minted)
    }

    /// The file's text. `edited` writes minted ids, raising `nbformat_minor` to 5.
    pub fn serialize(&self, edited: bool) -> String {
        let write_ids = edited && self.minted_ids();
        let mut root = self.root.clone();
        root.set(
            "cells",
            Json::Arr(self.cells.iter().map(|c| c.serialize(write_ids)).collect()),
        );
        root.set("metadata", self.metadata.clone());
        let minor = if write_ids {
            self.nbformat_minor.max(5)
        } else {
            self.nbformat_minor
        };
        if root.get("nbformat_minor").and_then(Json::as_i64) != Some(minor) {
            root.set("nbformat_minor", Json::Num(minor.to_string()));
        }
        json::write(&root, self.style)
    }

    /// The kernel's language, from `metadata.kernelspec.language` or `language_info.name`.
    pub fn language(&self) -> Option<String> {
        let md = &self.metadata;
        md.get("kernelspec")
            .and_then(|k| k.get("language"))
            .or_else(|| md.get("language_info").and_then(|l| l.get("name")))
            .and_then(Json::as_str)
            .map(str::to_string)
    }
}

const JSON_MIME_SUFFIX: &str = "+json";

fn is_json_mime(mime: &str) -> bool {
    mime == "application/json" || (mime.starts_with("application/") && mime.ends_with(JSON_MIME_SUFFIX))
}

/// The view of a raw output.
pub fn output_view(raw: &Json) -> Output {
    let text = |key: &str| raw.get(key).and_then(Json::as_multiline).unwrap_or_default();
    match raw.get("output_type").and_then(Json::as_str).unwrap_or("") {
        "stream" => Output::Stream {
            name: text("name"),
            text: text("text"),
        },
        "error" => Output::Error {
            ename: text("ename"),
            evalue: text("evalue"),
            traceback: raw
                .get("traceback")
                .and_then(Json::as_array)
                .unwrap_or(&[])
                .iter()
                .filter_map(|l| l.as_str().map(str::to_string))
                .collect(),
        },
        kind => {
            let bundle = match raw.get("data") {
                Some(Json::Obj(pairs)) => pairs
                    .iter()
                    .map(|(mime, v)| {
                        let value = if is_json_mime(mime) {
                            json::write(v, Style { indent: None, trailing_newline: false })
                        } else {
                            v.as_multiline().unwrap_or_else(|| {
                                json::write(v, Style { indent: None, trailing_newline: false })
                            })
                        };
                        (mime.clone(), value)
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Output::Data {
                execute_result: kind == "execute_result",
                execution_count: raw.get("execution_count").and_then(Json::as_i64),
                bundle,
            }
        }
    }
}

/// nbformat's `_split_mimebundle`: text types, JavaScript and SVG are written as lines, everything
/// else (images' base64, JSON) as it came.
fn split_bundle(data: &Json) -> Json {
    match data {
        Json::Obj(pairs) => Json::Obj(
            pairs
                .iter()
                .map(|(mime, v)| {
                    let split = mime.starts_with("text/")
                        || mime == "application/javascript"
                        || mime == "image/svg+xml";
                    let value = match v {
                        Json::Str(s) if split => Json::lines(s),
                        other => other.clone(),
                    };
                    (mime.clone(), value)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The nbformat output for an iopub message (`output_from_msg`), or `None` for a message that is
/// not an output. `content` is the message's parsed content.
pub fn output_from_message(msg_type: &str, content: &Json) -> Option<StoredOutput> {
    let field = |k: &str| content.get(k).cloned();
    let raw = match msg_type {
        "stream" => Json::sorted(vec![
            ("name".into(), field("name").unwrap_or(Json::str("stdout"))),
            ("output_type".into(), Json::str("stream")),
            (
                "text".into(),
                Json::lines(&content.get("text").and_then(Json::as_multiline).unwrap_or_default()),
            ),
        ]),
        "display_data" | "update_display_data" => Json::sorted(vec![
            ("data".into(), split_bundle(&field("data").unwrap_or(Json::Obj(Vec::new())))),
            ("metadata".into(), field("metadata").unwrap_or(Json::Obj(Vec::new()))),
            ("output_type".into(), Json::str("display_data")),
        ]),
        "execute_result" => Json::sorted(vec![
            ("data".into(), split_bundle(&field("data").unwrap_or(Json::Obj(Vec::new())))),
            ("execution_count".into(), field("execution_count").unwrap_or(Json::Null)),
            ("metadata".into(), field("metadata").unwrap_or(Json::Obj(Vec::new()))),
            ("output_type".into(), Json::str("execute_result")),
        ]),
        "error" => Json::sorted(vec![
            ("ename".into(), field("ename").unwrap_or(Json::str(""))),
            ("evalue".into(), field("evalue").unwrap_or(Json::str(""))),
            ("output_type".into(), Json::str("error")),
            ("traceback".into(), field("traceback").unwrap_or(Json::Arr(Vec::new()))),
        ]),
        _ => return None,
    };
    let display_id = content
        .get("transient")
        .and_then(|t| t.get("display_id"))
        .and_then(Json::as_str)
        .map(str::to_string);
    Some(StoredOutput { raw, display_id })
}

/// Append `output` to `outputs` the way Jupyter's frontends do: a stream following a stream of the
/// same name extends it rather than adding a second one.
pub fn push_output(outputs: &mut Vec<StoredOutput>, output: StoredOutput) {
    if let (Some(last), Output::Stream { name, text }) = (outputs.last_mut(), output.view())
        && let Output::Stream { name: last_name, text: last_text } = last.view()
        && last_name == name
    {
        last.raw.set("text", Json::lines(&(last_text + &text)));
        return;
    }
    outputs.push(output);
}
