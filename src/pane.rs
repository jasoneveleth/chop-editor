use im::OrdMap;

use crate::buffer::BufferId;
use crate::buffer::BufferOp;
use winit::keyboard::Key;
use winit::keyboard::NamedKey;
use winit::event::Modifiers;
use winit::keyboard::ModifiersKeyState;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert
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
    pub mode: Mode,
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
            mode: Mode::Normal,
        }
    }

    pub fn cursors_iter(&self) -> impl DoubleEndedIterator<Item = &Selection> {
        self.cursors.values()
    }

    pub fn insert(&self, k: Key, mods: &Modifiers) -> (Mode, Vec<BufferOp>) {
        match k {
            Key::Named(n) => {
                match n {
                    NamedKey::Enter => (Mode::Insert, vec![BufferOp::Insert(String::from("\n"))]),
                    NamedKey::ArrowLeft => (Mode::Insert, vec![BufferOp::MoveHorizontal(-1)]),
                    NamedKey::ArrowRight => (Mode::Insert, vec![BufferOp::MoveHorizontal(1)]),
                    NamedKey::ArrowUp => (Mode::Insert, vec![BufferOp::MoveVertical(-1)]),
                    NamedKey::ArrowDown => (Mode::Insert, vec![BufferOp::MoveVertical(1)]),
                    NamedKey::Space => (Mode::Insert, vec![BufferOp::Insert(String::from(" "))]),
                    NamedKey::Backspace => (Mode::Insert, vec![BufferOp::Delete]),
                    NamedKey::Escape => (Mode::Normal, vec![]),
                    _ => (Mode::Insert, vec![]),
                }
            },
            Key::Character(s) => {
                if !super_pressed(mods) {
                    (Mode::Insert, vec![BufferOp::Insert(String::from(s.as_str()))])
                } else {
                    let char = s.chars().nth(0).unwrap();
                    if char == 'w' {
                        (Mode::Insert, vec![BufferOp::Exit])
                    } else {
                        (Mode::Insert, vec![])
                    }
                }
            },
            _ => {
                unreachable!()
            }
        }
    }

    pub fn normal(&self, k: Key, mods: &Modifiers) -> (Mode, Vec<BufferOp>) {
        println!("normal!! {:?}", k);
        match k {
            Key::Named(n) => {
                match n {
                    NamedKey::ArrowLeft => (Mode::Normal, vec![BufferOp::MoveHorizontal(-1)]),
                    NamedKey::ArrowRight => (Mode::Normal, vec![BufferOp::MoveHorizontal(1)]),
                    NamedKey::ArrowUp => (Mode::Normal, vec![BufferOp::MoveVertical(-1)]),
                    NamedKey::ArrowDown => (Mode::Normal, vec![BufferOp::MoveVertical(1)]),
                    _ => (Mode::Normal, vec![]),
                }
            },
            Key::Character(s) => {
                let char = s.chars().nth(0).unwrap();
                match char {
                    'w' => {
                        if super_pressed(mods) {
                            (Mode::Normal, vec![BufferOp::Exit])
                        } else {
                            (Mode::Normal, vec![])
                        }
                    },
                    'h' => (Mode::Normal, vec![BufferOp::MoveHorizontal(-1)]),
                    'l' => (Mode::Normal, vec![BufferOp::MoveHorizontal(1)]),
                    'k' => (Mode::Normal, vec![BufferOp::MoveVertical(-1)]),
                    'j' => (Mode::Normal, vec![BufferOp::MoveVertical(1)]),
                    'i' => (Mode::Insert, vec![]),
                    'q' => (Mode::Normal, vec![BufferOp::Exit]),
                    _ => {
                        (Mode::Normal, vec![])
                    }
                }
            },
            _ => {
                unreachable!()
            }
        }
    }

    pub fn key(&self, key: Key, mods: &Modifiers) -> (Self, Vec<BufferOp>) {
        let (mode, ops) = match self.mode {
            Mode::Normal => {
                self.normal(key, mods)
            },
            Mode::Insert => {
                self.insert(key, mods)
            },
        };
        let cursors = self.cursors.clone();
        (Self { mode, cursors, ..*self}, ops)
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

fn super_pressed(m: &Modifiers) -> bool {
    m.lsuper_state() == ModifiersKeyState::Pressed || m.rsuper_state() == ModifiersKeyState::Pressed
}

