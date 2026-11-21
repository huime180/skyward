//! Persistent navigation beside the existing recovery pages on large tablets.
use crate::app::{App, Screen, Touch};
use crate::canvas::Canvas;
use crate::color::{mix, Rgba};
use crate::font::Fonts;
use crate::geom::Rect;
use crate::icons::Icon;
use crate::lang::Lang;
use crate::pages::{BLUE, GREEN, ORANGE, PURPLE, RED, SLATE};
use crate::theme::Theme;
use crate::widgets::{self, Row, Trail};

const SECTIONS: &[(Screen, [&str; 3], Icon)] = &[
    (Screen::About, ["我的设备", "我的裝置", "My device"], Icon::Phone),
    (Screen::Search, ["搜索", "搜尋", "Search"], Icon::More),
    (Screen::Wlan, ["WLAN", "Wi-Fi", "Wi-Fi"], Icon::Wlan),
    (Screen::Install, ["安装", "安裝", "Install"], Icon::Install),
    (Screen::Systems, ["双系统", "雙系統", "Dual boot"], Icon::Systems),
    (Screen::Backup, ["备份", "備份", "Backup"], Icon::Backup),
    (Screen::Restore, ["恢复", "還原", "Restore"], Icon::Restore),
    (Screen::Mount, ["挂载", "掛載", "Mount"], Icon::Mount),
    (Screen::Decrypt, ["解密", "解密", "Decrypt"], Icon::Lock),
    (Screen::Wipe, ["清除数据", "清除資料", "Wipe data"], Icon::Wipe),
    (Screen::Advanced, ["高级", "進階", "Advanced"], Icon::More),
    (Screen::Settings, ["设置", "設定", "Settings"], Icon::Gear),
    (Screen::Reboot, ["重启", "重新啟動", "Reboot"], Icon::Power),
];

pub(crate) fn is_section(screen: Screen) -> bool {
    SECTIONS.iter().any(|(s, _, _)| *s == screen)
}

pub enum Input {
    Detail(Touch),
    Select(Screen),
    Sidebar,
}

#[derive(Clone, Copy)]
enum Gesture {
    Idle,
    Detail,
    Sidebar { start: f32, scroll: f32, pick: Option<Screen>, moved: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SidebarKey {
    dark: bool,
    lang: Lang,
    selected: Screen,
    wlan: bool,
    systems: bool,
    scroll: u32,
    pressed: Option<Screen>,
}

pub struct Tablet {
    pub detail: Canvas,
    sidebar: Canvas,
    width: u32,
    height: u32,
    dp: f32,
    scroll: f32,
    max_scroll: f32,
    hits: Vec<(Rect, Screen)>,
    gesture: Gesture,
    cached: Option<SidebarKey>,
}

impl Tablet {
    pub fn new(width: u32, detail_width: u32, height: u32, dp: f32) -> Self {
        Self {
            detail: Canvas::new(detail_width, height), sidebar: Canvas::new(width, height),
            width, height, dp, scroll: 0.0, max_scroll: 0.0, hits: Vec::new(),
            gesture: Gesture::Idle, cached: None,
        }
    }

    pub fn cancel_gesture(&mut self) {
        self.gesture = Gesture::Idle;
    }

    pub fn paint(&mut self, out: &mut Canvas, f: &Fonts, info: &skyward_info::DeviceInfo, app: &App) {
        let selected = app.tablet_selection();
        let l = app.state.lang;
        let wlan = app.state.offers(Screen::Wlan);
        let systems = app.state.offers(Screen::Systems);
        let pressed = match self.gesture {
            Gesture::Sidebar { pick, moved: false, .. } => pick,
            _ => None,
        };
        let key = SidebarKey { dark: app.state.dark, lang: l, selected, wlan, systems,
            scroll: self.scroll.round() as u32, pressed };
        if self.cached != Some(key) {
            let t = if app.state.dark { Theme::dark(self.width, self.height, self.dp) }
                else { Theme::light(self.width, self.height, self.dp) };
            let d = self.dp;
            let c = &mut self.sidebar;
            c.clear(t.pal.bg);
            c.text_top(&f.title, "Skyward", 28.0*d, 94.0*d, 30.0*d, t.pal.text, None);
            let search = Rect::new(18.0*d, 148.0*d, self.width as f32 - 36.0*d, 42.0*d);
            widgets::search_bar(c, &t, search, 1.0);
            let words = widgets::search_words(&t, f, search,
                l.among("搜索功能与分区", "搜尋功能與分區", "Search recovery"));
            c.lay(&words, search.x.floor() as i32, search.y.floor() as i32, 1.0);
            let top = 202.0*d;
            self.hits.clear();
            self.hits.push((search, Screen::Search));
            // The sidebar holds the home page's own cards, a group at a
            // time, and scrolls them as one.
            let mut list = Canvas::band(self.width, self.height, top as u32..self.height, 0..self.width, t.pal.bg);
            let mut y = top - self.scroll;
            let mut hits = Vec::new();
            let sub = match &info.os_version {
                Some(v) => format!("{} · {v}", info.codename),
                None => info.codename.clone(),
            };
            let card = widgets::device_card(&mut list, &t, f, y, &info.model, &sub, &[]);
            hits.push((card.rows[0], Screen::About));
            y = card.bottom + t.m.card_gap;
            let hint = |screen: Screen| -> Option<String> {
                match screen {
                    Screen::Wlan => app.state.wlan.as_ref().map(|w| crate::fmt::wlan_value(l, w).to_string()),
                    Screen::Systems => match &app.state.systems {
                        Some(Ok(sys)) => Some(match &sys.other {
                            None => l.among("未安装第二系统", "未安裝第二系統", "No second system").to_string(),
                            Some(_) => sys.installed.name.clone(),
                        }),
                        _ => None,
                    },
                    Screen::Decrypt => Some(if matches!(app.state.data, crate::app::Data::Open) {
                        l.among("已解密", "已解密", "Decrypted").to_string()
                    } else {
                        l.among("未解密", "未解密", "Not decrypted").to_string()
                    }),
                    _ => None,
                }
            };
            let group = |s: Screen| match s {
                Screen::Wlan | Screen::Systems => 0,
                Screen::Install | Screen::Backup | Screen::Restore | Screen::Mount | Screen::Decrypt => 1,
                _ => 2,
            };
            let entries: Vec<_> = SECTIONS.iter().filter(|(s, _, _)| *s != Screen::Search && *s != Screen::About
                && (*s != Screen::Wlan || wlan) && (*s != Screen::Systems || systems)).collect();
            let mut entries = entries;
            // A card is one group: what belongs together stands together,
            // in the order the home page keeps them.
            entries.sort_by_key(|e| group(e.0));
            let hints: Vec<Option<String>> = entries.iter().map(|e| hint(e.0)).collect();
            let mut first = 0;
            while first < entries.len() {
                let mut end = first + 1;
                while end < entries.len() && group(entries[end].0) == group(entries[first].0) { end += 1; }
                let mut rows = Vec::new();
                for (index, &&(screen, title, icon)) in entries[first..end].iter().enumerate() {
                    let color = match screen {
                        Screen::Install => GREEN,
                        Screen::Restore | Screen::Systems => PURPLE,
                        Screen::Wipe => RED,
                        Screen::Reboot => ORANGE,
                        Screen::Wlan | Screen::Backup | Screen::Decrypt => BLUE,
                        _ => SLATE,
                    };
                    let mut row = Row::new(l.among(title[0], title[1], title[2])).icon(color, icon);
                    if let Some(hint) = hints[first + index].as_deref() {
                        row = row.trail(Trail::Value(hint));
                    }
                    rows.push(row);
                }
                let block = widgets::list(&mut list, &t, f, y, &rows);
                for (rect, &&(screen, _, _)) in block.rows.iter().zip(&entries[first..end]) {
                    hits.push((*rect, screen));
                }
                y = block.bottom + t.m.card_gap;
                first = end;
            }
            self.max_scroll = (y + self.scroll - top - 24.0*d).max(0.0);
            self.scroll = self.scroll.clamp(0.0, self.max_scroll);
            for (rect, screen) in hits {
                if rect.y + rect.h <= top || rect.y >= self.height as f32 { continue; }
                self.hits.push((Rect::new(rect.x, rect.y.max(top), rect.w,
                    (rect.y + rect.h).min(self.height as f32) - rect.y.max(top)), screen));
            }
            c.replace(&list);
            c.line(self.width as f32 - 1.0, 32.0*d, self.width as f32 - 1.0, self.height as f32 - 24.0*d,
                1.0, mix(t.pal.bg, t.pal.subtitle, 0.2));
            if self.max_scroll > 0.0 {
                let available = self.height as f32 - top - 16.0*d;
                let thumb = available * available / (available + self.max_scroll);
                let at = top + (available - thumb) * self.scroll / self.max_scroll;
                c.fill_round_rect(self.width as f32 - 5.0*d, at, 2.0*d, thumb, d, t.pal.chevron);
            }
            self.cached = Some(key);
        }
        out.clear(Rgba::rgb(0, 0, 0));
        out.lay(&self.sidebar, 0, 0, 1.0);
        out.lay(&self.detail, self.width as i32, 0, 1.0);
    }

    #[cfg(test)]
    pub fn section_center(&self, screen: Screen) -> (f32, f32) {
        self.hits.iter().find(|(_, s)| *s == screen).unwrap().0.centre()
    }

    /// A gesture keeps the pane it began in, including when crossing the divider.
    pub fn route(&mut self, event: Touch) -> Input {
        if let Touch::Down(x, y) = event {
            self.gesture = if x >= self.width as f32 { Gesture::Detail } else {
                let pick = self.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, s)| *s);
                Gesture::Sidebar { start: y, scroll: self.scroll, pick, moved: false }
            };
        }
        let result = match (self.gesture, event) {
            (Gesture::Detail, Touch::Down(x, y)) => Input::Detail(Touch::Down(x-self.width as f32, y)),
            (Gesture::Detail, Touch::Move(x, y)) => Input::Detail(Touch::Move(x-self.width as f32, y)),
            (Gesture::Detail, Touch::Up) => Input::Detail(Touch::Up),
            (Gesture::Sidebar { start, scroll, pick, moved }, Touch::Move(x, y)) => {
                let moved = moved || (y-start).abs() > 8.0*self.dp || x >= self.width as f32 || x < 0.0;
                self.scroll = (scroll + start - y).clamp(0.0, self.max_scroll);
                self.gesture = Gesture::Sidebar { start, scroll, pick, moved };
                Input::Sidebar
            }
            (Gesture::Sidebar { pick: Some(s), moved: false, .. }, Touch::Up) => Input::Select(s),
            _ => Input::Sidebar,
        };
        if event == Touch::Up { self.gesture = Gesture::Idle; }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_the_divider_never_changes_a_gestures_pane() {
        let mut t = Tablet::new(100, 200, 200, 1.0);
        t.hits.push((Rect::new(0.0, 0.0, 100.0, 100.0), Screen::Wipe));
        assert!(matches!(t.route(Touch::Down(30.0, 30.0)), Input::Sidebar));
        assert!(matches!(t.route(Touch::Move(180.0, 30.0)), Input::Sidebar));
        assert!(matches!(t.route(Touch::Up), Input::Sidebar));
        assert!(matches!(t.route(Touch::Down(130.0, 30.0)), Input::Detail(Touch::Down(30.0, 30.0))));
        assert!(matches!(t.route(Touch::Move(20.0, 30.0)), Input::Detail(Touch::Move(-80.0, 30.0))));
        assert!(matches!(t.route(Touch::Up), Input::Detail(Touch::Up)));
    }

    #[test]
    fn scrolling_cannot_select_a_section() {
        let mut t = Tablet::new(100, 200, 200, 1.0);
        t.hits.push((Rect::new(0.0, 0.0, 100.0, 100.0), Screen::Settings));
        t.route(Touch::Down(30.0, 30.0));
        assert!(matches!(t.route(Touch::Up), Input::Select(Screen::Settings)));
        t.route(Touch::Down(30.0, 30.0));
        t.route(Touch::Move(30.0, 60.0));
        assert!(matches!(t.route(Touch::Up), Input::Sidebar));
    }
}
