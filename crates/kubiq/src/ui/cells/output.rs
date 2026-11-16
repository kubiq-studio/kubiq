//! A cell's output, drawn: selectable, copyable text with a zoom button, and images decoded once.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use gpui::{
    AnyElement, App, Global, ImageFormat, InteractiveElement as _, StatefulInteractiveElement as _, IntoElement, MouseButton, ObjectFit,
    ParentElement as _, Styled as _, StyledImage as _, div, img, px,
};
use gpui_component::IconName;
use gpui_component::text::TextView;

use crate::theme::{self, Family, Role};
use crate::ui::kit;

/// Lines of a text output drawn inline; the rest is behind the zoom. The *Cells* preference
/// (`CellsSettings::output_lines`), set by the viewer that draws ([`set_inline_lines`]).
static INLINE_LINES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(400);

pub fn set_inline_lines(lines: usize) {
    INLINE_LINES.store(lines.max(10), std::sync::atomic::Ordering::Relaxed);
}

fn inline_lines() -> usize {
    INLINE_LINES.load(std::sync::atomic::Ordering::Relaxed)
}

/// Images decoded from base64, by the hash of the text, so a frame decodes nothing.
#[derive(Default)]
struct Images(HashMap<u64, Option<Arc<gpui::Image>>>);

impl Global for Images {}

fn hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// The image a base64 payload decodes to, cached.
pub fn image(cx: &mut App, mime: &str, base64: &str) -> Option<Arc<gpui::Image>> {
    let key = hash(base64);
    let cache = cx.default_global::<Images>();
    if let Some(found) = cache.0.get(&key) {
        return found.clone();
    }
    let format = match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/svg+xml" => ImageFormat::Svg,
        _ => return None,
    };
    let bytes = if mime == "image/svg+xml" {
        Some(base64.as_bytes().to_vec())
    } else {
        kubiq_notebook::output::base64_decode(base64)
    };
    let decoded = bytes.map(|b| Arc::new(gpui::Image::from_bytes(format, b)));
    if cache.0.len() > 512 {
        cache.0.clear();
    }
    cache.0.insert(key, decoded.clone());
    decoded
}

/// A fence that the text itself does not close.
fn fenced(text: &str) -> String {
    let mut fence = "```".to_string();
    while text.contains(&fence) {
        fence.push('`');
    }
    format!("{fence}text\n{text}\n{fence}")
}

/// The zoom button, top right of a block, shown on hover.
fn zoom_button(id: String, title: String, facts: String, text: String) -> AnyElement {
    kit::icon_button_tip(
        gpui::ElementId::Name(format!("{id}-zoom").into()),
        IconName::Maximize,
        "Zoom (F)",
        true,
        move |_, _, cx| {
            super::zoom::open_text(cx, title.clone(), facts.clone(), text.clone(), None);
        },
    )
    .absolute()
    .top_1()
    .right_1()
    .size(px(22.))
    .bg(theme::surface_raised())
    .border_1()
    .border_color(theme::border())
    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    .into_any_element()
}

/// Terminal text (already cleaned), selectable and copyable, with its zoom. `tone` colours it
/// (`theme::danger()` for stderr and tracebacks).
pub fn text_block(id: String, title: String, facts: String, text: String, tone: gpui::Rgba) -> AnyElement {
    let cap = inline_lines();
    let shown: String = if text.lines().count() > cap {
        let mut s: String = text.lines().take(cap).collect::<Vec<_>>().join("\n");
        s.push_str(&format!("\n… {} more lines: zoom to read all", text.lines().count() - cap));
        s
    } else {
        text.clone()
    };
    let group = format!("{id}-group");
    div()
        .id(gpui::ElementId::Name(id.clone().into()))
        .relative()
        .group(group)
        .w_full()
        .text_color(tone)
        .cursor_text()
        .child(
            TextView::markdown(gpui::ElementId::Name(format!("{id}-text").into()), fenced(&shown))
                .selectable(true)
                .text_size(theme::font(Family::Content, Role::Body)),
        )
        .child(zoom_button(id, title, facts, text))
        .into_any_element()
}

/// Rendered markdown (a markdown cell, a `text/markdown` output), selectable.
pub fn markdown_block(id: String, source: String) -> AnyElement {
    div()
        .w_full()
        .cursor_text()
        .child(
            TextView::markdown(gpui::ElementId::Name(id.into()), source)
                .selectable(true)
                .text_size(theme::font(Family::Content, Role::Body)),
        )
        .into_any_element()
}

/// An image, at most `max_height` tall inline; a click zooms it to fit the window.
pub fn image_block(id: String, title: String, image: Arc<gpui::Image>, max_height: f32) -> AnyElement {
    let zoomed = image.clone();
    div()
        .id(gpui::ElementId::Name(id.into()))
        .cursor_pointer()
        .max_h(px(max_height))
        .child(img(image).max_h(px(max_height)).object_fit(ObjectFit::ScaleDown))
        .on_click(move |_, _, cx| {
            super::zoom::open_image(cx, title.clone(), "image".into(), zoomed.clone());
        })
        .into_any_element()
}

/// The placeholder for an output with no native renderer: its MIME types and plain text.
pub fn placeholder(id: String, mimes: &[&str], plain: Option<&str>) -> AnyElement {
    let mut col = div().flex().flex_col().gap_1().child(
        div()
            .text_size(theme::font(Family::Content, Role::Meta))
            .text_color(theme::text_faint())
            .child(format!("[{}] — not drawn natively", mimes.join(", "))),
    );
    if let Some(plain) = plain {
        col = col.child(text_block(id, "Output".into(), mimes.join(", "), plain.to_string(), theme::text()));
    }
    col.into_any_element()
}
