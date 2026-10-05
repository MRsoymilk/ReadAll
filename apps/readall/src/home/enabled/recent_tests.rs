use super::recent_rows::marquee_offset;
use super::*;
use crate::test_font;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "readall-recent-ui-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn home<'a>(&self, font: &'a UiFont, names: &[&str], width: u32, height: u32) -> Home<'a> {
        let store = RecentStore::new(self.0.join("state"));
        for name in names.iter().rev() {
            let book = self.0.join(name);
            fs::write(&book, b"original book").unwrap();
            store.record(&book).unwrap();
        }
        let mut home = Home::new(width, height, font).unwrap();
        // All writes are explicitly isolated; never mutate the caller's real history.
        home.recent = store.load().unwrap();
        home.recent_store = Some(store);
        home.recent_rows.clear();
        home.paint().unwrap();
        home
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn font() -> UiFont {
    UiFont::from_bytes(test_font::make_font(), PathBuf::from("fixture.ttf")).unwrap()
}
fn center(rect: Rect) -> (i32, i32) {
    (
        rect.x + rect.width as i32 / 2,
        rect.y + rect.height as i32 / 2,
    )
}
fn click_delete(home: &mut Home<'_>, index: usize) {
    let (x, y) = center(home.recent_delete_rect(index));
    home.action(Action::Click { x, y }).unwrap();
    home.action(Action::PointerRelease { x, y }).unwrap();
}

#[test]
fn delete_button_removes_only_history_and_updates_visible_rows_without_opening() {
    let temp = Temp::new();
    let font = font();
    let names = ["first.epub", "second.mobi", "third.AZW3", "fourth.azw3"];
    let mut home = temp.home(&font, &names, 1040, 700);
    let keep = temp.0.join("state/reading-data.keep");
    fs::write(&keep, b"positions and notes").unwrap();
    let button = home.recent_delete_rect(1);
    let (x, y) = center(button);
    home.action(Action::PointerMove { x, y }).unwrap();
    assert_eq!(home.hover_target(), HoverTarget::RecentDelete(1));
    for i in 0..3 {
        let row = home.recent_row_rect(i);
        assert_eq!(
            home.surface.pixel((row.x + 8) as u32, (row.y + 1) as u32),
            Some(if i == 1 {
                home.theme.palette().hover
            } else {
                home.theme.palette().panel
            })
        );
    }
    home.action(Action::Click { x, y }).unwrap();
    assert_eq!(home.recent.len(), 4, "delete on release, not press");
    home.action(Action::PointerRelease { x, y }).unwrap();
    assert_eq!(
        home.recent,
        vec![
            temp.0.join(names[0]),
            temp.0.join(names[2]),
            temp.0.join(names[3])
        ]
    );
    assert_eq!(
        RecentStore::new(temp.0.join("state")).load().unwrap(),
        home.recent
    );
    assert!(home.selected_book.is_none());
    assert!(!home.close_requested());
    assert_eq!(home.recent_rows[2].path, temp.0.join(names[3]));
    assert_eq!(home.hover_target(), HoverTarget::None);
    // A duplicate release must not delete the newly shifted row.
    home.action(Action::PointerRelease { x, y }).unwrap();
    assert_eq!(home.recent.len(), 3);
    while !home.recent.is_empty() {
        click_delete(&mut home, 0);
    }
    assert_eq!(home.animation_interval(), None);
    assert!(home.recent_rows.is_empty());
    assert!(
        RecentStore::new(temp.0.join("state"))
            .load()
            .unwrap()
            .is_empty()
    );
    for name in names {
        assert_eq!(fs::read(temp.0.join(name)).unwrap(), b"original book");
    }
    assert_eq!(fs::read(keep).unwrap(), b"positions and notes");
    assert!(!home.close_requested());
}

#[test]
fn drag_out_leave_resize_or_changed_row_cancels_pending_delete() {
    let temp = Temp::new();
    let font = font();
    let mut home = temp.home(&font, &["one.epub", "two.azw3"], 1040, 700);
    let (x, y) = center(home.recent_delete_rect(0));
    home.action(Action::Click { x, y }).unwrap();
    home.action(Action::PointerMove { x: 300, y }).unwrap();
    home.action(Action::PointerRelease { x, y }).unwrap();
    assert_eq!(home.recent.len(), 2);
    home.action(Action::Click { x, y }).unwrap();
    home.action(Action::PointerLeave).unwrap();
    home.action(Action::PointerRelease { x, y }).unwrap();
    assert_eq!(home.recent.len(), 2);
    home.action(Action::Click { x, y }).unwrap();
    home.resize(1000, 700).unwrap();
    home.action(Action::PointerRelease { x, y }).unwrap();
    assert_eq!(home.recent.len(), 2);
    let (x, y) = center(home.recent_delete_rect(0));
    home.action(Action::Click { x, y }).unwrap();
    home.recent.swap(0, 1);
    home.paint().unwrap();
    home.action(Action::PointerRelease { x, y }).unwrap();
    assert_eq!(home.recent.len(), 2);
    assert!(home.selected_book.is_none());
    assert!(!home.close_requested());
}

#[test]
fn failed_persistence_retains_the_visible_list_and_reports_an_error() {
    let temp = Temp::new();
    let font = font();
    let mut home = temp.home(&font, &["one.epub", "two.mobi"], 1040, 700);
    let state = temp.0.join("state/recent-linux-v1.state");
    fs::write(&state, b"broken state\n").unwrap();
    let before = home.recent.clone();
    click_delete(&mut home, 0);
    assert_eq!(home.recent, before);
    assert!(home.status.starts_with("移除最近阅读失败"));
    assert_eq!(fs::read(state).unwrap(), b"broken state\n");
    assert!(home.selected_book.is_none());
    assert!(!home.close_requested());
    assert!(home.pending_recent_delete.is_none());
    home.recent_store = None;
    click_delete(&mut home, 0);
    assert_eq!(home.recent, before);
    assert!(home.status.contains("存储不可用"));
}

#[test]
fn long_names_animate_only_inside_the_name_clip_and_keep_delete_stationary() {
    let temp = Temp::new();
    let font = font();
    let long = format!("中文{}-end.azw3", "AW".repeat(70));
    let mut home = temp.home(&font, &[&long, "short.epub"], 1040, 700);
    assert!(home.recent_rows[0].overflow > 0);
    assert_eq!(home.recent_rows[1].overflow, 0);
    assert_eq!(
        home.recent_rows[0].name, long,
        "do not abbreviate the source filename"
    );
    let clip = home.recent_rows[0].clip;
    let button = home.recent_delete_rect(0);
    assert!(clip.x + (clip.width as i32) < button.x);
    let before = home.surface.pixels().to_vec();
    let now = home.recent_rows[0].started + Duration::from_secs(2);
    assert!(home.tick_recent_rows(now).unwrap());
    assert_eq!(home.recent_rows[0].offset, 28);
    let width = home.surface.width() as usize;
    let mut changed = 0;
    for (i, (a, b)) in before.iter().zip(home.surface.pixels()).enumerate() {
        if a != b {
            changed += 1;
            assert!(
                point_in(clip, (i % width) as i32, (i / width) as i32),
                "changed pixels outside filename at {i}"
            );
        }
    }
    assert!(changed > 0);
    assert_eq!(home.recent_delete_rect(0), button);
    assert!(
        !home.tick_recent_rows(now).unwrap(),
        "same time does not redraw"
    );
    // Normal body click still opens the original exact path after scrolling.
    home.action(Action::Click {
        x: clip.x + 5,
        y: clip.y + 5,
    })
    .unwrap();
    assert_eq!(home.selected_book, Some(temp.0.join(long)));
    assert_eq!(home.animation_interval(), None);
}

#[test]
fn marquee_pauses_at_both_ends_then_moves_back_without_time_step_dependence() {
    for (seconds, expected) in [
        (0.0, 0),
        (0.9, 0),
        (1.0, 0),
        (3.0, 56),
        (5.0, 112),
        (5.8, 112),
        (6.0, 112),
        (8.0, 56),
        (10.0, 0),
        (10.9, 0),
        (12.0, 28),
    ] {
        assert_eq!(
            marquee_offset(112, Duration::from_secs_f64(seconds)),
            expected
        );
    }
    assert_eq!(marquee_offset(0, Duration::from_secs(1000)), 0);
    for n in 0..1000 {
        assert!(marquee_offset(1, Duration::from_millis(n * 10)) <= 1);
    }
}

#[test]
fn animation_resets_on_width_changes_and_stops_in_browser_or_for_short_names() {
    let temp = Temp::new();
    let font = font();
    let mut surface = Surface::new(1, 1, RenderLimits::default()).unwrap();
    let painter = UiPainter::new(&font, &mut surface).unwrap();
    let name = (1..100)
        .map(|n| format!("{}.epub", "W".repeat(n)))
        .find(|s| painter.measure(14, s).unwrap() > 320)
        .unwrap();
    assert!(painter.measure(14, &name).unwrap() < 530);
    let mut home = temp.home(&font, &[&name], 680, 700);
    assert!(home.animation_interval().is_some());
    home.recent_rows[0].started = Instant::now() - Duration::from_secs(2);
    assert!(home.animation_tick().unwrap());
    assert!(home.recent_rows[0].offset > 0);
    home.resize(1040, 700).unwrap();
    assert_eq!(home.recent_rows[0].offset, 0);
    assert_eq!(home.animation_interval(), None);
    assert!(!home.animation_tick().unwrap());
    home.resize(680, 700).unwrap();
    assert_eq!(home.recent_rows[0].offset, 0);
    assert!(home.animation_interval().is_some());
    home.mode = Mode::Browser(Browser::load(temp.0.clone()).unwrap());
    assert_eq!(home.animation_interval(), None);
    assert!(!home.animation_tick().unwrap());
}

#[test]
fn minimum_window_has_no_hidden_or_overlapping_delete_targets() {
    let temp = Temp::new();
    let font = font();
    let mut home = temp.home(&font, &["one.epub", "two.mobi", "three.azw3"], 680, 460);
    assert_eq!(home.visible_recent_count(), 1);
    for height in [460, 490, 540, 700] {
        home.resize(680, height).unwrap();
        let count = home.visible_recent_count();
        assert_eq!(home.recent_rows.len(), count);
        for i in 0..count {
            let button = home.recent_delete_rect(i);
            let (x, y) = center(button);
            assert_eq!(home.hover_target_at(x, y), HoverTarget::RecentDelete(i));
            assert!(button.y + button.height as i32 <= height as i32 - 54);
            let clip = home.recent_rows[i].clip;
            assert!(clip.x + (clip.width as i32) < button.x);
        }
        for i in count..3 {
            let (x, y) = center(home.recent_delete_rect(i));
            assert_eq!(home.hover_target_at(x, y), HoverTarget::None);
        }
    }
}
