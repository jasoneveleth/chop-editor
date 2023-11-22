use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;
use std::fs::read_to_string;

// let | be the cursor, and \ be the end of the selection

// "|abc\djk" start: 0, offset: 3
// "|\abcdjk" start: 0, offset: 0
// "ab\cdj|k" start: 5, offset: -3

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Selection {
    // `start` is the location of the cursor is in the selection
    // 0 means before the first character, and n means after the nth character
    // emojis count as 1
    pub start: usize,
    // offset to the end of the selection such that `start + offset` is end of selection
    offset: i64,
}

pub struct FileInfo {
    pub filename: Rc<PathBuf>,
    // whether we've modified the buffer since `file_time`
    pub is_modified: bool,
    // last time that the file and buffer were identical 
    // (used to check if other processes have changed it)
    pub file_time: SystemTime,
}

pub struct TextBuffer {
    pub file: Option<FileInfo>,
    // !!! this should always be sorted
    pub cursors: Rc<[Selection]>,
    // this is the index
    pub main_cursor: usize,
    pub contents: Rc<str>,
}

impl Selection {
    pub fn reverse(&self) -> Self {
        Self {start: self.end(), offset: -self.offset}
    }

    pub fn end(&self) -> usize {
        return ((self.start as i64) - self.offset) as usize
    }
}

impl TextBuffer {
    pub fn from_filename(filename: &str) -> Result<Self, std::io::Error> {
        let contents = Rc::from(read_to_string(filename)?);
        let now = std::time::SystemTime::now();
        let filename = Rc::from(PathBuf::from(filename));
        let fi = FileInfo {filename, is_modified: false, file_time: now};
        let cursors = Rc::from([Selection{start: 0, offset: 0}]);
        Ok(Self {file: Some(fi), cursors, main_cursor: 0, contents})
    }

    // pub fn arrow(&self, )

    pub fn delete(&self) -> Self {
        let file = if let Some(fileinfo) = &self.file {
            Some(FileInfo {
                filename: fileinfo.filename.clone(),
                is_modified: true,
                file_time: fileinfo.file_time,
            })
        } else {
            None
        };
        let update = if self.cursors[self.main_cursor].offset == 0 {
            // adding 1 in zero case fixes it
            |(i, s): (usize, &Selection)| Selection{start: s.start + (s.start == 0) as usize - (i+1), offset: 0 as i64}
        } else {
            |(_, s): (usize, &Selection)| Selection{start: s.end(), offset: 0 as i64}
        };
        let cursors: Vec<Selection> = self.cursors.iter().enumerate().map(update).collect();
        let cursors = Rc::from(cursors);

        let mut contents = String::new();
        let mut prev = 0;
        for selection in self.cursors.iter() {
            // adding 1 in zero case fixes it
            contents += &self.contents[prev..selection.start + (selection.start==0) as usize -1];
            prev = selection.start;
        }
        contents += &self.contents[prev..];
        let contents = Rc::from(contents);

        Self {file, cursors, main_cursor: self.main_cursor, contents}
    }

    // pub fn up(&self) -> Self {
    //     let cursors: Vec<Selection> = self.cursors.iter().
    // }

    pub fn insert(&self, text: &str) -> Self {
        let file = if let Some(fileinfo) = &self.file {
            Some(FileInfo {
                filename: fileinfo.filename.clone(),
                is_modified: true,
                file_time: fileinfo.file_time,
            })
        } else {
            None
        };
        let cursors: Vec<Selection> = self.cursors.iter().enumerate().map(|(i, s)| Selection{start: s.start + (i+1) * text.len(), offset: text.len() as i64}).collect();
        let cursors = Rc::from(cursors);

        let mut contents = String::new();
        let mut prev = 0;
        for selection in self.cursors.iter() {
            contents += &self.contents[prev..selection.start];
            contents += text;
            prev = selection.start;
        }
        contents += &self.contents[prev..];
        let contents = Rc::from(contents);

        Self {file, cursors, main_cursor: self.main_cursor, contents}
    }

    pub fn lines(&self) -> std::str::Lines {
        self.contents.lines()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_buffer_insertion() {
        let buffer = TextBuffer {
            file: None,
            cursors: Rc::from(vec![Selection {start: 1, offset: 1}, Selection {start: 5, offset: 1}, Selection {start: 8, offset: 1}]),
            contents: Rc::from("abcdefghigh"),
            main_cursor: 0,
        };
        let buffer = buffer.insert("xz");

        // Assert that the buffer is created as expected
        assert_eq!(buffer.contents.as_ref(), "axzbcdexzfghxzigh");
        for (a, b) in buffer.cursors.iter().zip(&[Selection{start: 3, offset: 2}, Selection{start: 9, offset: 2}, Selection{start: 14, offset: 2}]) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn test_lines() {
        let buffer = TextBuffer {
            file: None,
            cursors: Rc::from(vec![Selection {start: 1, offset: 1}, Selection {start: 5, offset: 1}, Selection {start: 8, offset: 1}]),
            contents: Rc::from("abcdef\njfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh"),
            main_cursor: 0,
        };
        let v = vec!["abcdef", "jfkdsalfjads", "kadsjlfla", "alskdjflasd", "asdjkflsda", "aghigh"];

        for (a, b) in buffer.lines().zip(v) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn test_delete() {
        let buffer = TextBuffer {
            file: None,
            cursors: Rc::from(vec![Selection {start: 1, offset: 0}, Selection {start: 5, offset: 0}, Selection {start: 8, offset: 0}]),
            contents: Rc::from("abcdef\njfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh"),
            main_cursor: 0,
        };
        let buffer = buffer.delete();

        assert_eq!(buffer.contents.as_ref(), "bcdf\nfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh");
        for (a, b) in buffer.cursors.iter().zip(&[Selection{start: 0, offset: 0}, Selection{start: 3, offset: 0}, Selection{start: 5, offset: 0}]) {
            assert_eq!(a, b);
        }
    }
}
