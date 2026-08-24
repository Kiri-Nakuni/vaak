use std::fs;
use std::path::PathBuf;

#[test]
fn c_headerは固定幅assertと実装済みexportだけを持つ() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("include")
        .join("iron_vaak_v0.h");
    let header = fs::read_to_string(path).expect("headerを読む");
    for width in ["== 64", "== 56", "== 48", "== 32", "== 8"] {
        assert!(header.contains(width), "{width} のstatic assert");
    }
    assert!(header.contains("offsetof(IronVaakHostLayoutEntryV0, name_offset) == 32"));
    assert!(header.contains("iron_vaak_v0_prepare("));
    assert!(header.contains("iron_vaak_v0_runner_report_patch_copy("));
    assert!(header.contains("sizeof(IronVaakRunnerReportInfoV0) == 64"));
    assert!(!header.contains("UnityEngine"));
    assert!(!header.contains("lua_State"));
}
