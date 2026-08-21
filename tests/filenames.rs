//! 版方のファイル名が**どの環境でも開けるか。**
//!
//! Windows は `: < > " | ? *` を名前に使えず、末尾の `.` と空白も落とす。
//! **一つ紛れ込むだけで、その環境では clone すらできない。**
//!
//! 事故で `:=` という名前の空ファイルが入っていた（`legacy/` の中）。
//! **見つけるのが人であってはいけない。**

use std::process::Command;

fn tracked() -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .output()
        .expect("git が要る");
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn 使えない文字を含まない() {
    let files = tracked();
    let bad: Vec<_> = files
        .iter()
        .filter(|p| p.chars().any(|c| ":<>\"|?*".contains(c) || (c as u32) < 0x20))
        .collect();
    assert!(bad.is_empty(), "Windows で開けない名前: {bad:?}");
}

#[test]
fn 末尾が点や空白でない() {
    let files = tracked();
    let bad: Vec<_> = files
        .iter()
        .filter(|p| {
            p.split('/')
                .any(|part| part.ends_with('.') || part.ends_with(' '))
        })
        .collect();
    assert!(bad.is_empty(), "末尾が `.` か空白: {bad:?}");
}

#[test]
fn 予約名を使わない() {
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
        "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let files = tracked();
    let bad: Vec<_> = files
        .iter()
        .filter(|p| {
            p.split('/').any(|part| {
                let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
                RESERVED.contains(&stem.as_str())
            })
        })
        .collect();
    assert!(bad.is_empty(), "Windows の予約名: {bad:?}");
}

#[test]
fn 大文字小文字だけ違う組が無い() {
    // **macOS と Windows は区別しない。** 二つ入れると片方が消える
    let files = tracked();
    let mut seen: std::collections::HashMap<String, &String> = std::collections::HashMap::new();
    let mut bad = Vec::new();
    for f in &files {
        let key = f.to_lowercase();
        if let Some(other) = seen.insert(key, f) {
            bad.push((other.clone(), f.clone()));
        }
    }
    assert!(bad.is_empty(), "大文字小文字だけ違う: {bad:?}");
}
