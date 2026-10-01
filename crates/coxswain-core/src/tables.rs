//! Files that hold tables, as both apps show them: a SQLite database's tables with their row
//! counts, schema and first rows, and a Parquet file's columns and first rows. F3 in the
//! terminal app pages them as text; the desktop app's preview draws the same.

use std::fs::File;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Rows shown of each table.
pub const ROWS: usize = 20;
/// Characters shown of a value.
const CELL: usize = 60;

#[derive(Serialize, Debug)]
pub struct Table {
    pub name: String,
    /// "table" or "view".
    pub kind: String,
    /// `None` for views, or when counting took too long.
    pub rows: Option<u64>,
    pub sql: String,
    /// The column names, then the first rows.
    pub sample: Vec<Vec<String>>,
}

/// A value cut to what a cell shows, on one line.
fn cell(s: &str) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).take(CELL + 1).collect();
    if one.chars().count() > CELL { format!("{}…", one.chars().take(CELL - 1).collect::<String>()) } else { one }
}

/// Tables and views with their row counts, schema and first `rows` rows. Opened read-only;
/// counting and reading stop after about a second per table on huge databases.
pub fn sqlite(path: &Path, rows: usize) -> Result<Vec<Table>, String> {
    use rusqlite::types::ValueRef;
    use rusqlite::{Connection, OpenFlags};
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|e| e.to_string())?;
    let mut tables: Vec<Table> = db
        .prepare("SELECT name, type, coalesce(sql, '') FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY type, name")
        .and_then(|mut q| q.query_map([], |r| Ok(Table { name: r.get(0)?, kind: r.get(1)?, rows: None, sql: r.get(2)?, sample: vec![] }))?.collect())
        .map_err(|e| e.to_string())?;
    for t in &mut tables {
        let name = format!("\"{}\"", t.name.replace('"', "\"\""));
        let start = std::time::Instant::now();
        // Without the hook, counting a huge table simply has no time limit.
        let _ = db.progress_handler(10_000, Some(move || start.elapsed().as_millis() > 1000));
        if t.kind == "table" {
            t.rows = db.query_row(&format!("SELECT count(*) FROM {name}"), [], |r| r.get::<_, i64>(0)).ok().map(|n| n as u64);
        }
        let Ok(mut q) = db.prepare(&format!("SELECT * FROM {name} LIMIT {rows}")) else { continue };
        t.sample.push(q.column_names().iter().map(|c| cell(c)).collect());
        let n = q.column_count();
        let Ok(mut it) = q.query([]) else { continue };
        while let Ok(Some(row)) = it.next() {
            t.sample.push(
                (0..n)
                    .map(|i| match row.get_ref(i) {
                        Ok(ValueRef::Null) | Err(_) => String::new(),
                        Ok(ValueRef::Integer(v)) => v.to_string(),
                        Ok(ValueRef::Real(v)) => v.to_string(),
                        Ok(ValueRef::Text(v)) => cell(&String::from_utf8_lossy(v)),
                        Ok(ValueRef::Blob(v)) => format!("<{} bytes>", v.len()),
                    })
                    .collect(),
            );
        }
    }
    let _ = db.progress_handler(0, None::<fn() -> bool>);
    Ok(tables)
}

/// A Parquet file's row count, its columns (name, type), and its first rows or why not.
pub type Parquet = (u64, Vec<(String, String)>, Result<Vec<Vec<String>>, String>);

/// A Parquet file: its row count, its columns with their types, and the column names then the
/// first `rows` rows. The rows are missing (with why) when they are compressed in a way only
/// the desktop app reads (zstd).
pub fn parquet(path: &Path, rows: usize) -> Result<Parquet, String> {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    let r = SerializedFileReader::new(File::open(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let meta = r.metadata().file_metadata();
    let schema: Vec<(String, String)> = meta.schema_descr().columns().iter().map(|c| (c.path().string(), format!("{}", c.physical_type()))).collect();
    let count = meta.num_rows().max(0) as u64;
    let sample = (|| {
        let mut out = vec![schema.iter().map(|c| cell(&c.0)).collect::<Vec<_>>()];
        for row in r.get_row_iter(None).map_err(|e| e.to_string())?.take(rows) {
            let row = row.map_err(|e| e.to_string())?;
            out.push(row.get_column_iter().map(|(_, v)| cell(&if let parquet::record::Field::Str(s) = v { s.clone() } else { v.to_string() })).collect());
        }
        Ok(out)
    })()
    .map_err(|e: String| if e.contains("zstd") { crate::t!("preview.parquet_zstd") } else { e });
    Ok((count, schema, sample))
}

/// Whether F3 shows this file as tables.
pub fn is_tables(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    ["db", "sqlite", "sqlite3", "db3", "parquet", "pq"].contains(&ext.as_str())
}

/// Rows as aligned columns of text.
fn grid(rows: &[Vec<String>]) -> String {
    let width = |i: usize| rows.iter().filter_map(|r| r.get(i)).map(|c| c.chars().count()).max().unwrap_or(0);
    let widths: Vec<usize> = (0..rows.first().map_or(0, Vec::len)).map(width).collect();
    let mut out = String::new();
    for (n, row) in rows.iter().enumerate() {
        let line: Vec<String> = row.iter().zip(&widths).map(|(c, w)| format!("{c}{}", " ".repeat(w - c.chars().count()))).collect();
        out.push_str(line.join(" │ ").trim_end());
        out.push('\n');
        if n == 0 {
            out.push_str(&widths.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("─┼─"));
            out.push('\n');
        }
    }
    out
}

/// A table file as text to page through: what F3 shows in the terminal app.
pub fn as_text(path: &Path) -> Result<String, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if ext == "parquet" || ext == "pq" {
        let (count, schema, sample) = parquet(path, ROWS)?;
        let mut out = format!("{name}: {}\n\n", crate::tn!("preview.rows", count));
        let types: Vec<Vec<String>> = std::iter::once(vec![crate::t!("preview.column"), crate::t!("preview.type")]).chain(schema.into_iter().map(|(n, t)| vec![n, t])).collect();
        out += &grid(&types);
        out.push('\n');
        match sample {
            Ok(rows) => out += &grid(&rows),
            Err(why) => out += &format!("{why}\n"),
        }
        return Ok(out);
    }
    let mut out = String::new();
    for t in sqlite(path, ROWS)? {
        let rows = t.rows.map(|n| crate::tn!("preview.rows", n)).unwrap_or_default();
        out += &format!("{} {}{}\n", if t.kind == "view" { crate::t!("preview.view", "name" => t.name) } else { t.name.clone() }, if rows.is_empty() { "" } else { "· " }, rows);
        out += &format!("{}\n\n", t.sql);
        if t.sample.len() > 1 {
            out += &grid(&t.sample);
            out.push('\n');
        }
    }
    Ok(out)
}

/// `path` as text in a file of the cache, for the pager: F3.
pub fn text_copy(path: &Path) -> Result<PathBuf, String> {
    let text = as_text(path)?;
    let to = crate::archive::peek_folder().map_err(|e| e.to_string())?.join(format!("{}.txt", path.file_name().unwrap_or_default().to_string_lossy()));
    std::fs::write(&to, text).map_err(|e| e.to_string())?;
    Ok(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_show_a_database_with_its_first_rows() {
        let p = std::env::temp_dir().join(format!("coxswain-tables-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let db = rusqlite::Connection::open(&p).unwrap();
        db.execute_batch("CREATE TABLE \"we\"\"ird\"(id INTEGER, name TEXT, pic BLOB); INSERT INTO \"we\"\"ird\" VALUES (1, 'Ada', x'0102'), (2, NULL, NULL); CREATE VIEW v AS SELECT name FROM \"we\"\"ird\";").unwrap();
        drop(db);
        let t = sqlite(&p, 1).unwrap();
        assert_eq!(t.iter().map(|t| (t.name.as_str(), t.rows)).collect::<Vec<_>>(), [("we\"ird", Some(2)), ("v", None)]);
        assert_eq!(t[0].sample, [vec!["id", "name", "pic"], vec!["1", "Ada", "<2 bytes>"]], "the column names, then as many rows as asked");
        assert_eq!(t[1].sample[1], ["Ada"], "a view shows its rows too");
        let text = as_text(&p).unwrap();
        assert!(text.contains("id │ name │ pic") && text.contains("Ada"), "{text}");
        assert!(is_tables(&p) && !is_tables(Path::new("a.txt")));
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn tables_show_a_parquet_file() {
        use parquet::data_type::{ByteArray, ByteArrayType, Int64Type};
        use parquet::file::writer::SerializedFileWriter;
        let p = std::env::temp_dir().join(format!("coxswain-tables-{}.parquet", std::process::id()));
        let schema = std::sync::Arc::new(parquet::schema::parser::parse_message_type("message m { required int64 id; required binary city (UTF8); }").unwrap());
        let props = std::sync::Arc::new(parquet::file::properties::WriterProperties::builder().set_compression(parquet::basic::Compression::SNAPPY).build());
        let mut w = SerializedFileWriter::new(File::create(&p).unwrap(), schema, props).unwrap();
        let mut g = w.next_row_group().unwrap();
        let mut c = g.next_column().unwrap().unwrap();
        c.typed::<Int64Type>().write_batch(&[1, 2, 3], None, None).unwrap();
        c.close().unwrap();
        let mut c = g.next_column().unwrap().unwrap();
        c.typed::<ByteArrayType>().write_batch(&[ByteArray::from("Aarhus"), ByteArray::from("Odense"), ByteArray::from("Aalborg")], None, None).unwrap();
        c.close().unwrap();
        g.close().unwrap();
        w.close().unwrap();
        let (count, schema, sample) = parquet(&p, 2).unwrap();
        assert_eq!((count, schema[1].0.as_str()), (3, "city"));
        assert_eq!(sample.unwrap(), [vec!["id", "city"], vec!["1", "Aarhus"], vec!["2", "Odense"]]);
        assert!(as_text(&p).unwrap().contains("Odense"));
        std::fs::remove_file(p).unwrap();
    }
}
