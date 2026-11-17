//! What the explorer's *New* entries can create beyond an empty file: typed documents seeded with
//! a valid starting body. One row per kind; a kind adds its own here.
//!
//! The menu ([`super::menu::menu_entries`]) lists *New file*, *New folder*, the drawing entries and
//! every template flat while there are at most [`FLAT_LIMIT`] of them, and folds them all into one
//! *New ▸* stage beyond that. The created file is written by the host's file worker (create-only),
//! then opened in whatever viewer claims it.

/// One typed document the explorer can create.
#[derive(Clone, Copy, Debug)]
pub struct NewTemplate {
    /// The menu row: `New notebook`.
    pub label: &'static str,
    /// The extension the name is forced to carry, without the leading dot (`runbook.md`).
    pub extension: &'static str,
    /// The name the prompt is seeded with, before the extension.
    pub stem: &'static str,
    /// The file's first bytes.
    pub bytes: fn() -> Vec<u8>,
}

/// *New* entries the menu shows flat; one more and they fold into *New ▸*.
pub const FLAT_LIMIT: usize = 4;

fn notebook() -> Vec<u8> {
    // nbformat 4.5, one empty code cell, no kernelspec: the resolver picks the kernel.
    kubiq_notebook::Notebook::new_empty(0x6b75_6269_71).serialize().into_bytes()
}

fn runbook() -> Vec<u8> {
    kubiq_runbook::template().into_bytes()
}

/// Every template, in menu order.
pub fn templates() -> &'static [NewTemplate] {
    &[
        NewTemplate {
            label: "New notebook",
            extension: "ipynb",
            stem: "Untitled",
            bytes: notebook,
        },
        NewTemplate {
            label: "New runbook",
            extension: "runbook.md",
            stem: "Untitled",
            bytes: runbook,
        },
    ]
}

/// The template a forced extension names, if one does.
pub fn by_extension(extension: &str) -> Option<&'static NewTemplate> {
    templates()
        .iter()
        .find(|t| t.extension.eq_ignore_ascii_case(extension))
}

/// `stem.extension`, or `stem 2.extension`, `stem 3.extension`, … — the first `taken` says is free.
pub fn unique_name(stem: &str, extension: &str, taken: impl Fn(&str) -> bool) -> String {
    let first = format!("{stem}.{extension}");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{stem} {n}.{extension}"))
        .find(|name| !taken(name))
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_skip_taken_ones() {
        assert_eq!(unique_name("Untitled", "ipynb", |_| false), "Untitled.ipynb");
        let taken = ["Untitled.ipynb", "Untitled 2.ipynb"];
        assert_eq!(unique_name("Untitled", "ipynb", |n| taken.contains(&n)), "Untitled 3.ipynb");
    }

    #[test]
    fn the_notebook_template_parses() {
        let text = String::from_utf8(notebook()).unwrap();
        let nb = kubiq_notebook::Notebook::parse(&text, 1).unwrap();
        assert_eq!(nb.order().len(), 1);
        assert!(text.contains("\"nbformat_minor\": 5"));
    }
}
