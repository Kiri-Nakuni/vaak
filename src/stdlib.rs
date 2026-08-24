//! Vaak で書かれた任意選択のライブラリ。
//!
//! 中核の無名標準ライブラリ（S-3）とは別であり、利用するホストが必要な
//! ソースだけを利用者プログラムの前へ置く。

/// バイト単位の文字列操作を定義する Vaak ソース。
///
/// `str` への新しい組み込みを増やさず、`str_*` 自由関数として実装している。
/// 呼び出し側は、この文字列を利用者ソースの前へ連結してから解析する。
pub const STRING: &str = include_str!("../stdlib/string.vaak");

/// UTF-8 JSONをflat documentへparseし、compact JSONへserializeするVaak source。
///
/// [`STRING`]を先に利用者sourceへ連結する。JSON numberは初版では`i64`だけを
/// 表現し、正しい小数・指数表記はtyped unsupported errorとして返す。
pub const JSON_UTF8: &str = include_str!("../stdlib/codec/json_utf8.vaak");

/// 有界chunk readerと一行writerを定義するUTF-8 JSON LinesのVaak source。
///
/// [`STRING`]、[`JSON_UTF8`]、このsourceの順に連結する。file/socket capabilityは
/// 含まず、hostから渡されたbyte chunkと返却するowned `str`だけを扱う。
pub const JSONL_UTF8: &str = include_str!("../stdlib/codec/jsonl_utf8.vaak");

/// 固定長集合を32-bit wordへ詰めるpure Vaak source。
///
/// TeXの有限文字class、game snapshot内の有限mask、競技programの集合演算で
/// 同じ表現を使える。host capabilityや特定applicationの意味は含まない。
pub const DENSE_BITSET_U32: &str = include_str!("../stdlib/ds/dense_bitset_u32.vaak");
