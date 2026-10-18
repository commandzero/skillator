use super::model::Action;
use ratatui::layout::Rect;

/// Frame-local mouse targets produced by the renderer and consumed before the next redraw.
#[derive(Default)]
pub struct InteractionMap {
    pub area: Rect,
    pub detail_scroll_max: Option<u16>,
    targets: Vec<Target>,
    scroll_targets: Vec<ScrollTarget>,
}

struct Target {
    area: Rect,
    action: Action,
}

struct ScrollTarget {
    area: Rect,
    up: Action,
    down: Action,
}

impl InteractionMap {
    pub fn reset(&mut self, area: Rect) {
        self.area = area;
        self.detail_scroll_max = None;
        self.clear_targets();
    }

    pub fn clear_targets(&mut self) {
        self.targets.clear();
        self.scroll_targets.clear();
    }

    pub fn add(&mut self, area: Rect, action: Action) {
        if let Some(area) = clip(area, self.area) {
            self.targets.push(Target { area, action });
        }
    }

    pub fn add_scroll(&mut self, area: Rect, up: Action, down: Action) {
        if let Some(area) = clip(area, self.area) {
            self.scroll_targets.push(ScrollTarget { area, up, down });
        }
    }

    pub fn action_at(&self, column: u16, row: u16) -> Option<Action> {
        self.targets
            .iter()
            .rev()
            .find(|target| contains(target.area, column, row))
            .map(|target| target.action.clone())
    }

    pub fn scroll_at(&self, column: u16, row: u16, down: bool) -> Option<Action> {
        self.scroll_targets
            .iter()
            .rev()
            .find(|target| contains(target.area, column, row))
            .map(|target| {
                if down {
                    target.down.clone()
                } else {
                    target.up.clone()
                }
            })
    }
}

fn contains(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x
        && x < area.x.saturating_add(area.width)
        && y >= area.y
        && y < area.y.saturating_add(area.height)
}

fn clip(area: Rect, frame: Rect) -> Option<Rect> {
    let x = area.x.max(frame.x);
    let y = area.y.max(frame.y);
    let right = area
        .x
        .saturating_add(area.width)
        .min(frame.x.saturating_add(frame.width));
    let bottom = area
        .y
        .saturating_add(area.height)
        .min(frame.y.saturating_add(frame.height));
    if right <= x || bottom <= y {
        return None;
    }
    Some(Rect::new(x, y, right - x, bottom - y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::Action;

    #[test]
    fn offscreen_targets_are_inert_without_clipping_underflow() {
        let mut interactions = InteractionMap::default();
        interactions.reset(Rect::new(10, 10, 5, 5));
        interactions.add(Rect::new(20, 11, 3, 1), Action::MoveUp);
        interactions.add_scroll(Rect::new(10, 20, 3, 1), Action::MoveUp, Action::MoveDown);
        assert_eq!(interactions.action_at(20, 11), None);
        assert_eq!(interactions.scroll_at(10, 20, true), None);
    }
}
