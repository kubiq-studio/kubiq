//! The read-only zoom dialog every cell document opens on a cell's source or output: a text body
//! in a read-only editor — selection, copy, find, line numbers — or an image fitted to the window.
//! The markdown view's zoom (`ui::mdview::zoom`) in shape: near the window's size, scrim, ×,
//! outside click and Esc close it, Copy all in the header.
//!
//! **State is one `Global`**: [`open_text`] / [`open_image`] set it from any handler (they take
//! `&mut App`), and the viewer that hosts the dialog draws [`overlay`] last in its own render. The
//! editor is built at that first draw, because building one needs the `Window`.

use std::sync::Arc;

use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, Global, InteractiveElement as _,
    IntoElement, ObjectFit, ParentElement as _, Styled as _, StyledImage as _, Window, div, img, px,
};
use gpui_component::IconName;
use gpui_component::input::{Editor, EditorState};

use crate::app::AppState;
use crate::theme::{self, Family, Role};
use crate::ui::kit;

enum Body {
    Text {
        text: String,
        language: Option<&'static str>,
        editor: Option<Entity<EditorState>>,
    },
    Image(Arc<gpui::Image>),
}

struct Open {
    title: String,
    facts: String,
    body: Body,
}

#[derive(Default)]
struct Zoom {
    open: Option<Open>,
}

impl Global for Zoom {}

/// Open `text` read-only. `facts` is the header's second line (`cell 3 · stdout · 1 204 lines`).
pub fn open_text(cx: &mut App, title: String, facts: String, text: String, language: Option<&'static str>) {
    cx.default_global::<Zoom>().open = Some(Open {
        title,
        facts,
        body: Body::Text {
            text,
            language,
            editor: None,
        },
    });
    cx.refresh_windows();
}

pub fn open_image(cx: &mut App, title: String, facts: String, image: Arc<gpui::Image>) {
    cx.default_global::<Zoom>().open = Some(Open {
        title,
        facts,
        body: Body::Image(image),
    });
    cx.refresh_windows();
}

pub fn close(cx: &mut App) {
    cx.default_global::<Zoom>().open = None;
    cx.refresh_windows();
}

pub fn is_open(cx: &App) -> bool {
    cx.try_global::<Zoom>().is_some_and(|z| z.open.is_some())
}

/// The dialog, when one is open. Draw it last in the hosting view.
pub fn overlay(window: &mut Window, cx: &mut Context<AppState>) -> Option<AnyElement> {
    let zoom = cx.try_global::<Zoom>()?;
    zoom.open.as_ref()?;
    // The editor needs the window: built once, the first time the dialog is drawn.
    let needs_editor = matches!(
        zoom.open.as_ref().map(|o| &o.body),
        Some(Body::Text { editor: None, .. })
    );
    if needs_editor {
        let (text, language) = match &zoom.open.as_ref()?.body {
            Body::Text { text, language, .. } => (text.clone(), *language),
            Body::Image(_) => return None,
        };
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(true)
                .soft_wrap(false)
                .searchable(true)
                .default_value(text);
            if let Some(language) = language {
                state = state.language(language);
            }
            state
        });
        editor.update(cx, |state, cx| state.set_readonly(true, cx));
        let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
        window.focus(&focus, cx);
        if let Some(Open { body: Body::Text { editor: slot, .. }, .. }) =
            &mut cx.default_global::<Zoom>().open
        {
            *slot = Some(editor);
        }
    }
    let open = cx.global::<Zoom>().open.as_ref()?;
    let copy_text = match &open.body {
        Body::Text { text, .. } => Some(text.clone()),
        Body::Image(_) => None,
    };
    let body = match &open.body {
        Body::Text { editor, .. } => div()
            .flex_1()
            .min_h(px(0.))
            .size_full()
            .font_family(theme::MONO_FONT)
            .children(editor.as_ref().map(|e| Editor::new(e).into_any_element()))
            .into_any_element(),
        Body::Image(image) => div()
            .flex_1()
            .min_h(px(0.))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(img(image.clone()).size_full().object_fit(ObjectFit::Contain))
            .into_any_element(),
    };
    let body = div()
        .id("cells-zoom-body")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .on_key_down(|event, _window, cx| {
            if event.keystroke.key == "escape" {
                close(cx);
                cx.stop_propagation();
            }
        })
        .child(body)
        .into_any_element();
    let actions = copy_text.map(|text| {
        kit::icon_button_tip("cells-zoom-copy", IconName::Copy, "Copy all", true, move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        })
        .into_any_element()
    });
    let footer = div()
        .text_size(theme::font(Family::Chrome, Role::Label))
        .text_color(theme::text_faint())
        .child(format!(
            "{} \u{b7} Select text and press Cmd+C to copy a part \u{b7} Esc closes",
            open.facts
        ))
        .into_any_element();
    let viewport = window.viewport_size();
    Some(kit::modal_sized(
        "cells-zoom-dialog",
        theme::accent(),
        f32::from(viewport.width) * 0.94,
        Some(f32::from(viewport.height) * theme::MODAL_MAX_HEIGHT),
        &open.title,
        actions,
        body,
        footer,
        |_, cx| close(cx),
        window,
    ))
}
