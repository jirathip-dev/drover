mod common;
use common::tmpdir;
use drover::lanes::{read_tail, Tail};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

struct Counted(std::fs::File, u64);

impl Read for Counted {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        let n = self.0.read(b)?;
        self.1 += n as u64;
        Ok(n)
    }
}

impl Seek for Counted {
    fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(p)
    }
}

fn read(t: &mut Tail, p: &Path) -> u64 {
    let m = std::fs::metadata(p).unwrap();
    let mut f = Counted(std::fs::File::open(p).unwrap(), 0);
    let part = t.read(&mut f, m.ino(), m.len()).unwrap();
    assert_eq!(
        part.is_empty(),
        std::fs::read(p).unwrap().ends_with(b"\n"),
        "the unfinished line: {part}"
    );
    f.1
}

fn append(p: &Path, s: &str) {
    std::fs::OpenOptions::new()
        .append(true)
        .open(p)
        .unwrap()
        .write_all(s.as_bytes())
        .unwrap();
}

fn line(i: usize) -> String {
    format!("{{\"type\":\"user\",\"n\":{i},\"pad\":\"{}\"}}\n", "x".repeat(80))
}

#[test]
fn an_append_reads_only_the_new_bytes_and_a_partial_line_waits() {
    let d = tmpdir("tail");
    let p = d.join("s.jsonl");
    let first: String = (0..10).map(line).collect();
    std::fs::write(&p, &first).unwrap();
    let mut t = Tail::default();
    assert_eq!(read(&mut t, &p), first.len() as u64);
    append(&p, &line(10));
    assert_eq!(read(&mut t, &p), 64 + line(10).len() as u64);
    assert_eq!(t.text, first.clone() + &line(10));
    append(&p, "{\"half");
    assert_eq!(read(&mut t, &p), 64 + 6);
    assert_eq!(read_tail(&p, 1 << 21).unwrap(), first.clone() + &line(10) + "{\"half");
    assert_eq!(
        t.text,
        first.clone() + &line(10),
        "a line still being written is not taken"
    );
    append(&p, "\":1}\n");
    assert_eq!(read(&mut t, &p), 64 + 11);
    assert!(t.text.ends_with("{\"half\":1}\n"));
    assert_eq!(read(&mut t, &p), 64, "unchanged: only the check");
}

#[test]
fn truncation_rotation_or_a_changed_prefix_reads_the_whole_file() {
    let d = tmpdir("tail");
    let p = d.join("s.jsonl");
    let all: String = (0..10).map(line).collect();
    std::fs::write(&p, &all).unwrap();
    let mut t = Tail::default();
    read(&mut t, &p);
    let short: String = (0..3).map(line).collect();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_len(short.len() as u64)
        .unwrap();
    assert_eq!(read(&mut t, &p), short.len() as u64);
    assert_eq!(t.text, short);
    let rotated: String = (20..40).map(line).collect();
    let q = d.join("new.jsonl");
    std::fs::write(&q, &rotated).unwrap();
    std::fs::rename(&q, &p).unwrap();
    assert_eq!(read(&mut t, &p), rotated.len() as u64);
    assert_eq!(t.text, rotated);
    let mut f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
    f.seek(SeekFrom::Start(rotated.len() as u64 - 20)).unwrap();
    f.write_all(b"\"changed\"}\n").unwrap();
    append(&p, &line(40));
    let len = std::fs::metadata(&p).unwrap().len();
    assert_eq!(read(&mut t, &p), 64 + len);
    assert!(t.text.ends_with(&line(40)) && t.text.contains("changed"));
}

#[test]
fn a_1_mb_transcript_plus_a_400_byte_append_reads_under_2_kb() {
    let d = tmpdir("tail");
    let p = d.join("s.jsonl");
    let big: String = (0..(1 << 20) / line(0).len() + 1).map(line).collect();
    assert!(big.len() >= 1 << 20);
    std::fs::write(&p, &big).unwrap();
    let mut t = Tail::default();
    assert_eq!(read(&mut t, &p), big.len() as u64);
    let more = format!("{{\"type\":\"assistant\",\"pad\":\"{}\"}}\n", "y".repeat(400 - 30));
    assert_eq!(more.len(), 400);
    append(&p, &more);
    assert!(read(&mut t, &p) < 2048);
    assert!(t.text.ends_with(&more));
    assert_eq!(read_tail(&p, 1 << 21).unwrap(), big.clone() + &more);
    assert_eq!(read_tail(&p, 500).unwrap(), more);
}
