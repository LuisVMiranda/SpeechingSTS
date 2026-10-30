use super::*;
use eframe::egui::{self, Pos2, Rect};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = format!(
            "{}-{}",
            ChatRecord::new("English".into()).id,
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        );
        Self(std::env::temp_dir().join(format!("speeching-quick-chat-{id}")))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn test_app(directory: &TestDirectory, settings: Settings) -> (App, egui::Context) {
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let credentials = credentials::Store::empty_for_test();
    let mut app = App::with_settings_and_store(&creation, settings, credentials);
    app.store = ChatStore::for_test(directory.0.clone());
    app.conversation_view = ConversationView::for_test(directory.0.join("conversation-view.json"));
    (app, ctx)
}

fn save_chat(app: &App, title: &str) -> ChatRecord {
    let mut chat = ChatRecord::new("Spanish".into());
    chat.id = format!("{}-{}", chat.id, NEXT_ID.fetch_add(1, Ordering::Relaxed));
    chat.rename(title).unwrap();
    chat.sessions.push(SavedSession {
        voice: "Aoede".into(),
        duration_seconds: 12,
        messages: vec![MemoryMessage {
            speaker: Speaker::User,
            text: "Hola, how are you?".into(),
        }],
    });
    app.store.save(&chat).unwrap();
    chat
}

fn render(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<(String, Rect)> {
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(320.0, 330.0))),
        events,
        ..Default::default()
    };
    let mut frame = eframe::Frame::_new_kittest();
    let mut output = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut frame));
    let mut result = Vec::new();
    for shape in &output.shapes {
        collect_text(&shape.shape, &mut result);
    }
    output.textures_delta.clear();
    result
}

fn collect_text(shape: &egui::Shape, result: &mut Vec<(String, Rect)>) {
    match shape {
        egui::Shape::Text(text) => result.push((
            text.galley.job.text.clone(),
            Rect::from_min_size(text.pos, text.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_text(shape, result);
            }
        }
        _ => {}
    }
}

fn text_position(shapes: &[(String, Rect)], text: &str) -> Pos2 {
    text_bounds(shapes, text).center()
}

fn text_bounds(shapes: &[(String, Rect)], text: &str) -> Rect {
    shapes
        .iter()
        .find(|(value, _)| value == text)
        .unwrap_or_else(|| panic!("Missing {text:?} in {shapes:?}"))
        .1
}

fn click(app: &mut App, ctx: &egui::Context, pos: Pos2) -> Vec<(String, Rect)> {
    let event = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    render(app, ctx, vec![egui::Event::PointerMoved(pos), event(true)]);
    render(app, ctx, vec![event(false)])
}

#[cfg(windows)]
#[test]
fn main_window_switches_new_and_saved_chat_without_opening_the_log() {
    let directory = TestDirectory::new();
    let (mut app, ctx) = test_app(&directory, Settings::default());
    let saved = save_chat(&app, "Cafe practice");
    app.select_chat(&saved.id);

    let mut shapes = render(&mut app, &ctx, vec![]);
    click(
        &mut app,
        &ctx,
        text_position(&shapes, "Continue: Cafe practice"),
    );
    assert!(app.quick_chat_open);
    shapes = render(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, text_position(&shapes, "New conversation"));
    assert!(app.selected_chat.is_none());
    assert!(!app.log_open);
    assert_eq!(app.store.load(&saved.id).unwrap().context().0.len(), 1);

    shapes = render(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, text_position(&shapes, "New conversation"));
    shapes = render(&mut app, &ctx, vec![]);
    assert!(app.quick_chat_open);
    click(&mut app, &ctx, text_position(&shapes, "Cafe practice"));
    assert_eq!(app.selected_chat.as_ref().unwrap().id, saved.id);
    assert_eq!(app.selected_chat.as_ref().unwrap().context().0.len(), 1);
    assert!(!app.quick_chat_open);
    assert!(!app.log_open);
}

#[cfg(windows)]
#[test]
fn picker_filters_saved_chats_and_fits_the_minimum_window() {
    let directory = TestDirectory::new();
    let (mut app, ctx) = test_app(&directory, Settings::default());
    for index in 0..12 {
        save_chat(&app, &format!("Practice {index}"));
    }
    let shapes = render(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, text_position(&shapes, "New conversation"));
    assert!(app.quick_chat_open);
    let shapes = render(&mut app, &ctx, vec![]);
    let search = text_position(&shapes, "Search saved conversations");
    click(&mut app, &ctx, search);
    let shapes = render(&mut app, &ctx, vec![egui::Event::Text("Practice 1".into())]);
    assert_eq!(app.quick_chat_query, "Practice 1");
    assert!(shapes.iter().any(|(text, _)| text == "Practice 1"));
    assert!(shapes.iter().any(|(text, _)| text == "Practice 11"));
    assert!(!shapes.iter().any(|(text, _)| text == "Practice 3"));
    let new_choice = text_position(&shapes, "New conversation");
    assert!((0.0..330.0).contains(&new_choice.y));
    assert!((0.0..320.0).contains(&new_choice.x));
}

#[cfg(windows)]
#[test]
fn choosing_new_keeps_the_existing_selection_if_saving_fails() {
    let directory = TestDirectory::new();
    fs::write(&directory.0, "a file cannot hold conversation files").unwrap();
    let (mut app, _ctx) = test_app(&directory, Settings::default());
    let chat = ChatRecord::new("Spanish".into());
    let id = chat.id.clone();
    app.selected_chat = Some(chat);
    app.history_dirty = true;

    assert!(!app.apply_quick_choice(Choice::New));
    assert_eq!(app.selected_chat.as_ref().unwrap().id, id);
    assert!(app.error);
    app.history_dirty = false;
    fs::remove_file(&directory.0).unwrap();
}

#[cfg(windows)]
#[test]
fn empty_picker_is_localized_in_light_and_dark_minimal_views() {
    for language in UiLanguage::ALL {
        for theme in [Theme::Light, Theme::Dark] {
            let directory = TestDirectory::new();
            let settings = Settings {
                ui_language: language,
                theme,
                minimal: true,
                ..Settings::default()
            };
            let (mut app, ctx) = test_app(&directory, settings);
            let shapes = render(&mut app, &ctx, vec![]);
            assert!(shapes
                .iter()
                .all(|(text, _)| !text.contains(language.text("New conversation"))));
            click(&mut app, &ctx, Pos2::new(37.0, 37.0));
            let shapes = render(&mut app, &ctx, vec![]);
            assert!(app.quick_chat_open);
            for text in ["Continue a conversation", "No saved conversations yet."] {
                let position = text_position(&shapes, language.text(text));
                assert!((0.0..320.0).contains(&position.x));
                assert!((0.0..330.0).contains(&position.y));
            }
        }
    }
}

#[cfg(windows)]
#[test]
fn missing_saved_chat_is_reported_without_changing_selection() {
    let directory = TestDirectory::new();
    let (mut app, _ctx) = test_app(&directory, Settings::default());
    let current = save_chat(&app, "Current chat");
    let missing = save_chat(&app, "Missing chat");
    app.select_chat(&current.id);
    app.store.delete(&missing.id).unwrap();

    assert!(!app.apply_quick_choice(Choice::Saved(missing.id)));
    assert_eq!(app.selected_chat.as_ref().unwrap().id, current.id);
    assert!(app.error);
    assert!(!app.status.is_empty());
}

#[cfg(windows)]
#[test]
fn picker_shows_dates_and_can_change_and_remember_sort_order() {
    use super::super::library_view::SortOrder;
    let directory = TestDirectory::new();
    let (mut app, ctx) = test_app(&directory, Settings::default());
    let mut first = save_chat(&app, "Planning a holiday");
    first.updated_at = 1_700_000_000;
    app.store.save(&first).unwrap();
    let mut second = save_chat(&app, "Cooking at home");
    second.updated_at = 1_800_000_000;
    app.store.save(&second).unwrap();

    let shapes = render(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, text_position(&shapes, "New conversation"));
    let shapes = render(&mut app, &ctx, vec![]);
    assert!(shapes
        .iter()
        .any(|(text, _)| text == &date_time::compact(second.updated_at, UiLanguage::English)));
    click(&mut app, &ctx, text_position(&shapes, "Newest first"));
    let shapes = render(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, text_position(&shapes, "Oldest first"));
    render(&mut app, &ctx, vec![]);

    assert!(app.quick_chat_open);
    assert_eq!(app.conversation_view.order, SortOrder::Oldest);
    let restored = ConversationView::for_test(directory.0.join("conversation-view.json"));
    assert_eq!(restored.order, SortOrder::Oldest);
}

#[cfg(windows)]
#[test]
fn long_titles_leave_room_for_dates_in_the_small_picker() {
    for theme in [Theme::Light, Theme::Dark] {
        let directory = TestDirectory::new();
        let settings = Settings {
            theme,
            ..Settings::default()
        };
        let (mut app, ctx) = test_app(&directory, settings);
        let title = "A long conversation about favorite Italian recipes and travel plans";
        let mut chat = save_chat(&app, title);
        chat.updated_at = 1_800_000_000;
        app.store.save(&chat).unwrap();
        let shapes = render(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, text_position(&shapes, "New conversation"));
        let shapes = render(&mut app, &ctx, vec![]);
        let title_rect = text_bounds(&shapes, title);
        let date_rect = text_bounds(
            &shapes,
            &date_time::compact(chat.updated_at, UiLanguage::English),
        );
        assert!(
            title_rect.max.x <= date_rect.min.x,
            "title={title_rect:?}, date={date_rect:?}"
        );
        assert!(
            date_rect.max.x < 320.0 && date_rect.max.y < 330.0,
            "date={date_rect:?}"
        );
    }
}

#[cfg(windows)]
#[test]
fn detached_library_displays_full_dates_and_the_shared_sort_choice() {
    use super::super::library_view::SortOrder;
    let directory = TestDirectory::new();
    let (mut app, ctx) = test_app(&directory, Settings::default());
    let chat = save_chat(&app, "Discussing travel plans");
    app.refresh_library();
    app.conversation_view.order = SortOrder::Oldest;
    app.log_open = true;
    app.library_open = true;
    app.window_positions = WindowPositions::default();
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 700.0))),
        ..Default::default()
    };
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output.textures_delta.clear();
        output = ctx.run_ui(raw.clone(), |ui| app.log_panel(ui.ctx()));
    }
    app.positions_dirty = false;
    let mut shapes = Vec::new();
    for shape in &output.shapes {
        collect_text(&shape.shape, &mut shapes);
    }
    output.textures_delta.clear();
    let expected = format!(
        "Spanish · Last updated: {}",
        date_time::full(chat.updated_at, UiLanguage::English)
    );
    assert!(shapes.iter().any(|(text, _)| text == &expected));
    assert!(shapes.iter().any(|(text, _)| text == "Oldest first"));
}
