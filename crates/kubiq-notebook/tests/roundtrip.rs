//! Every fixture, in the shape JupyterLab, VS Code and Colab save, re-serialises byte for byte when
//! nothing was edited; edits touch only what they name.

use kubiq_notebook::doc::{CellOp, Notebook, Origin};
use kubiq_notebook::nbformat::Output;
use kubiq_notebook::{CellKind, output};

const FIXTURES: &[(&str, &str)] = &[
    ("jupyterlab", include_str!("fixtures/jupyterlab.ipynb")),
    ("vscode", include_str!("fixtures/vscode.ipynb")),
    ("colab", include_str!("fixtures/colab.ipynb")),
    ("empty-no-newline", include_str!("fixtures/empty-no-newline.ipynb")),
];

#[test]
fn unedited_fixtures_are_byte_identical() {
    for (name, text) in FIXTURES {
        let nb = Notebook::parse(text, 7).unwrap_or_else(|e| panic!("{name}: {e}"));
        let out = nb.serialize();
        if out != *text {
            for (n, (a, b)) in text.lines().zip(out.lines()).enumerate() {
                assert_eq!(a, b, "{name}: line {}", n + 1);
            }
            panic!("{name}: lengths differ ({} vs {})", text.len(), out.len());
        }
    }
}

#[test]
fn an_edit_changes_only_its_cell_and_writes_minted_ids() {
    let text = FIXTURES[2].1; // colab: no ids, unsorted, two-space
    let mut nb = Notebook::parse(text, 7).unwrap();
    let first = nb.order()[0].clone();
    nb.apply(
        vec![CellOp::Source {
            id: first,
            source: "Données\nmore".into(),
        }],
        Origin::Human,
    )
    .unwrap();
    let out = nb.serialize();
    assert!(out.contains("\"nbformat_minor\": 5"));
    assert!(out.contains("        \"Données\\n\",\n        \"more\"\n"));
    // Ids are appended to Colab's unsorted cells, after their last key.
    assert!(out.contains("      \"metadata\": {\n        \"id\": \"Xy12Ab\"\n      },\n      \"id\": \""));
    // The untouched cell keeps its output and source verbatim.
    assert!(out.contains("            \"hello\\n\"\n"));
    let again = Notebook::parse(&out, 9).unwrap();
    assert_eq!(again.order(), nb.order(), "the written ids are read back");
}

#[test]
fn a_string_source_stays_a_string() {
    let mut nb = Notebook::parse(FIXTURES[1].1, 1).unwrap();
    let id = nb.order()[0].clone();
    nb.apply(vec![CellOp::Source { id, source: "x = 2\ny = 3".into() }], Origin::Human)
        .unwrap();
    assert!(nb.serialize().contains("\"source\": \"x = 2\\ny = 3\""));
}

#[test]
fn outputs_read_as_views() {
    let nb = Notebook::parse(FIXTURES[0].1, 1).unwrap();
    let views = nb.views();
    assert_eq!(views.len(), 4);
    assert_eq!(views[0].kind, CellKind::Markdown);
    let Output::Stream { text, .. } = &views[1].outputs[0] else {
        panic!("a stream")
    };
    // `\r` returns to the start of the line and overwrites, as a terminal does.
    assert_eq!(output::terminal_text(text).lines().next(), Some("doneing"));
    let Output::Data { bundle, .. } = &views[2].outputs[0] else {
        panic!("display data")
    };
    let (mime, b64) = output::richest(bundle).unwrap();
    assert_eq!(mime, "image/png");
    assert!(output::base64_decode(b64).unwrap().starts_with(b"\x89PNG"));
    assert!(matches!(views[2].outputs[1], Output::Error { .. }));
}

#[test]
fn a_large_notebook_reads() {
    // Well over the editor's 2 MiB read cap: a document the host owns has none.
    let blob = "A".repeat(4 * 1024 * 1024);
    let text = format!(
        "{{\n \"cells\": [\n  {{\n   \"cell_type\": \"code\",\n   \"execution_count\": 1,\n   \"id\": \"x\",\n   \"metadata\": {{}},\n   \"outputs\": [\n    {{\n     \"data\": {{\n      \"image/png\": \"{blob}\"\n     }},\n     \"metadata\": {{}},\n     \"output_type\": \"display_data\"\n    }}\n   ],\n   \"source\": []\n  }}\n ],\n \"metadata\": {{}},\n \"nbformat\": 4,\n \"nbformat_minor\": 5\n}}\n"
    );
    let nb = Notebook::parse(&text, 1).unwrap();
    assert_eq!(nb.serialize(), text);
}
