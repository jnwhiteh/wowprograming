use std::path::PathBuf;
use wowprogramming_formats::db2::Table;

/// Prints records of a DB2 file: PROBE_FDID=<id> cargo test -p wowprogramming-formats --test probe -- --ignored --nocapture
#[test]
#[ignore]
fn probe_db2() {
    let dir = PathBuf::from("/mnt/d/World of Warcraft");
    let s = wowprogramming_casc::Storage::open(&dir, "wow").unwrap();
    let id: u32 = std::env::var("PROBE_FDID").unwrap().parse().unwrap();
    let data = s.read_file(id).unwrap();
    let t = Table::parse(&data).unwrap();
    println!("records {} fields {}", t.records.len(), t.field_count());
    for r in t.records.iter().take(8) {
        let strs: Vec<String> = (0..t.field_count())
            .map(|f| t.string(r, f).chars().take(40).collect())
            .collect();
        println!("{} {:?} {:?}", r.id, r.values, strs);
    }
}
