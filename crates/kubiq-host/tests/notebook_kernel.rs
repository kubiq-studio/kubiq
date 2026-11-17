//! A real Jupyter kernel through the host's own wire protocol: start, run, outputs, interrupt,
//! shutdown. Ignored by default — it needs a Python with `ipykernel`:
//!
//! ```text
//! KUBIQ_TEST_PYTHON=/path/to/venv/bin/python cargo test -p kubiq-host --test notebook_kernel -- --ignored
//! ```

use std::time::{Duration, Instant};

use kubiq_host::notebook::kernel::{self, Channel, KernelEvent, Launch};
use kubiq_notebook::doc::Notebook;
use kubiq_notebook::json::Json;
use kubiq_notebook::nbformat::{Output, output_from_message};
use kubiq_notebook::output;

fn python() -> Option<String> {
    std::env::var("KUBIQ_TEST_PYTHON").ok().filter(|p| !p.is_empty())
}

/// Run `code` and collect its outputs into a notebook cell until the run's idle and reply.
fn run(k: &mut kernel::Kernel, events: &flume::Receiver<kernel::Tagged>, nb: &mut Notebook, cell: &str, code: &str) -> (String, Option<i64>) {
    let msg_id = k.execute(code).unwrap();
    nb.run_started(cell, code);
    let (mut idle, mut status, mut count) = (false, None, None);
    let until = Instant::now() + Duration::from_secs(60);
    while (!idle || status.is_none()) && Instant::now() < until {
        let Ok((_, KernelEvent::Message(channel, msg))) = events.recv_timeout(Duration::from_secs(5)) else {
            continue;
        };
        if msg.parent_id() != msg_id {
            continue;
        }
        match (channel, msg.msg_type()) {
            (Channel::Iopub, "status") => {
                idle = msg.content.get("execution_state").and_then(Json::as_str) == Some("idle")
            }
            (Channel::Shell, "execute_reply") => {
                status = msg.content.get("status").and_then(Json::as_str).map(str::to_string);
                count = msg.content.get("execution_count").and_then(Json::as_i64);
            }
            (Channel::Iopub, kind) => {
                if let Some(out) = output_from_message(kind, &msg.content) {
                    nb.push_output(cell, out);
                }
            }
            _ => {}
        }
    }
    (status.unwrap_or_default(), count)
}

#[test]
#[ignore = "needs KUBIQ_TEST_PYTHON pointing at a Python with ipykernel"]
fn a_real_kernel_runs_cells() {
    let Some(python) = python() else {
        eprintln!("KUBIQ_TEST_PYTHON is not set; skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("kubiq-nb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (tx, events) = flume::unbounded();
    let spec = kubiq_notebook::env::synthesize(&python, "test");
    let mut k = kernel::start(
        Launch {
            spec,
            cwd: dir.clone(),
            runtime_dir: dir.join("runtime"),
        },
        1,
        tx,
    )
    .expect("the kernel starts");
    assert_eq!(k.language.as_deref(), Some("python"));

    let mut nb = Notebook::new_empty(1);
    let cell = nb.order()[0].clone();

    let (status, count) = run(&mut k, &events, &mut nb, &cell, "print(1+1)\n40 + 2");
    assert_eq!(status, "ok");
    assert_eq!(count, Some(1));
    let view = nb.view(&cell).unwrap();
    assert_eq!(view.outputs[0], Output::Stream { name: "stdout".into(), text: "2\n".into() });
    let Output::Data { execute_result: true, bundle, .. } = &view.outputs[1] else {
        panic!("an execute_result: {:?}", view.outputs)
    };
    assert_eq!(bundle[0], ("text/plain".to_string(), "42".to_string()));

    // A PNG display, no matplotlib: a 1x1 image from bytes.
    let png = "import base64\nfrom IPython.display import Image, display\ndisplay(Image(data=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGBgAAAABQABpfZFQAAAAABJRU5ErkJggg==')))";
    let (status, _) = run(&mut k, &events, &mut nb, &cell, png);
    assert_eq!(status, "ok");
    let view = nb.view(&cell).unwrap();
    let Output::Data { bundle, .. } = &view.outputs[0] else { panic!("display data") };
    let (mime, b64) = output::richest(bundle).unwrap();
    assert_eq!(mime, "image/png");
    assert!(output::base64_decode(b64).unwrap().starts_with(b"\x89PNG"));

    // An error.
    let (status, _) = run(&mut k, &events, &mut nb, &cell, "1/0");
    assert_eq!(status, "error");
    assert!(matches!(nb.view(&cell).unwrap().outputs[0], Output::Error { ref ename, .. } if ename == "ZeroDivisionError"));

    // Interrupt a sleeping cell.
    let msg_id = k.execute("import time\ntime.sleep(30)").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    k.interrupt().unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    let mut interrupted = false;
    while Instant::now() < until && !interrupted {
        if let Ok((_, KernelEvent::Message(Channel::Shell, msg))) = events.recv_timeout(Duration::from_secs(1))
            && msg.parent_id() == msg_id
        {
            interrupted = msg.content.get("status").and_then(Json::as_str) == Some("error");
        }
    }
    assert!(interrupted, "the interrupt ended the run");

    // The serialized notebook carries the outputs in nbformat's shape.
    assert!(nb.serialize().contains("\"ename\": \"KeyboardInterrupt\"") || nb.serialize().contains("ZeroDivisionError"));

    k.shutdown();
    assert!(std::fs::read_dir(dir.join("runtime")).unwrap().next().is_none(), "the connection file is gone");
    let _ = std::fs::remove_dir_all(&dir);
}
