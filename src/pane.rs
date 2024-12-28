use im::OrdMap;

use crate::buffer::BufferId;

// let | be the cursor, and \ be the end of the selection

// "|abc\djk" start: 0, offset: 3
// "|\abcdjk" start: 0, offset: 0
// "ab\cdj|k" start: 5, offset: -3

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Selection {
    // `start` is the location of the cursor is in the selection
    // 0 means before the first byte, and n means after the nth character
    pub start: usize,
    // offset to the end of the selection such that `start + offset` is end of selection
    pub offset: i64,
}

impl Selection {
    pub fn reverse(&self) -> Self {
        Self {start: self.end(), offset: -self.offset}
    }

    pub fn end(&self) -> usize {
        return ((self.start as i64) + self.offset) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.offset == 0
    }
}

pub type PaneId = usize;

#[derive(Debug, Clone)]
pub struct Pane {
    // !!! this should always be sorted
    pub cursors: OrdMap<usize, Selection>,
    // this is the index (byte offset)
    pub main_cursor_start: usize,
    // keep track so up/down will maintain rough position
    pub grapheme_col_offset: usize,
    pub buffer_id: BufferId,
    pub id: PaneId,
    pub y_offset: f32,
}

impl Pane {
    pub fn new(
        buffer_id: BufferId,
        pane_id: PaneId,
    ) -> Self {
        let mut cursors = OrdMap::new();
        let start = 0;
        cursors.insert(start, Selection {start, offset: 0});
        Self {
            cursors,
            main_cursor_start: 0,
            grapheme_col_offset: 0,
            buffer_id,
            id: pane_id,
            y_offset: 0.,
        }
    }

    pub fn cursors_iter(&self) -> impl DoubleEndedIterator<Item = &Selection> {
        self.cursors.values()
    }

    pub fn scroll_y(&self, y: f32, end: f32) -> Self {
        let y_offset = (self.y_offset + y).max(0.).min(end);
        Pane {
            y_offset,
            cursors: self.cursors.clone(),
            ..*self
        }

    }
}
