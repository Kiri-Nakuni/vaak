//! 位置情報。ホストは paradox の発生点を受け取る（C-46）ので、
//! 値にもエラーにも位置が要る。

/// ソース内のバイト範囲。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// 二つの範囲を覆う範囲。
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    /// 何も指さない範囲。組み込みの定義など、ソースに対応が無いもの。
    pub const NONE: Span = Span { start: 0, end: 0 };
}

/// 行と桁（1 始まり）。エラー表示のためだけに使う。
pub fn line_col(src: &str, offset: u32) -> (usize, usize) {
    let offset = offset as usize;
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in src.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
