//! Lot 7.5: the memory `decode_answer` uses for an answer of almost 16 MiB stays below the size
//! of the body, whatever its shape.
//!
//! A counting `#[global_allocator]` needs `unsafe impl GlobalAlloc`, which the workspace denies
//! (`unsafe_code`, decision of John: no new `unsafe`). The peak is read instead from the kernel:
//! the resident peak of the process (`VmHWM` of `/proc/self/status`), reset before each decoding
//! by writing `5` to `/proc/self/clear_refs`. Linux only; this file holds one test, so that no
//! other test of the binary runs at the same time.

#![cfg(target_os = "linux")]

use std::fs;

use sermofur_daemon::framing::{self, MAX_RESPONSE_BYTES};

/// Resident peak of this process, in bytes, since the last reset.
fn peak() -> usize {
    let status = fs::read_to_string("/proc/self/status").expect("the status of the process");
    let line = status
        .lines()
        .find(|line| line.starts_with("VmHWM:"))
        .expect("a resident peak");
    let kib: usize = line
        .trim_start_matches("VmHWM:")
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .expect("a number of KiB");
    kib * 1024
}

/// Resident size now, in bytes.
fn resident() -> usize {
    let status = fs::read_to_string("/proc/self/status").expect("the status of the process");
    let line = status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .expect("a resident size");
    line.trim_start_matches("VmRSS:")
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse::<usize>()
        .expect("a number of KiB")
        * 1024
}

/// The memory a decoding adds to the resident size at its peak.
fn added_by(decode: impl FnOnce()) -> usize {
    fs::write("/proc/self/clear_refs", "5").expect("the peak is reset");
    let before = resident();
    decode();
    peak().saturating_sub(before)
}

/// An answer of `size` bytes: a 4 MiB `stdout`, then an unknown array nested to the limit of
/// 64 and an unknown object, both huge (the body of `framing_tests.rs`).
fn nested(size: usize) -> String {
    let item = r#"{"a":[1,-2.5e3,"xé😀",null,true,false,{}],"b":[]},"#;
    let mut body = String::with_capacity(size);
    body.push_str(r#"{"kind":"result","id":7,"exitCode":0,"stdout":""#);
    body.push_str(&"o".repeat(4 * 1024 * 1024));
    body.push_str(r#"","deep":"#);
    body.push_str(&"[".repeat(60));
    while body.len() < size / 2 + 2 * 1024 * 1024 {
        body.push_str(item);
    }
    body.push('0');
    body.push_str(&"]".repeat(60));
    body.push_str(r#","other":{"#);
    let mut object = 0;
    while body.len() < size - 1024 {
        body.push_str(&format!(r#""k{object}":{item}"#));
        object += 1;
    }
    body.push_str(r#""last":0}}"#);
    body
}

/// An answer of `size` bytes made of empty unknown names: `{"kind":"result","":0,"":0,…}`.
fn flat(size: usize) -> String {
    let mut body = String::with_capacity(size);
    body.push_str(r#"{"kind":"result""#);
    while body.len() < size - 8 {
        body.push_str(r#","":0"#);
    }
    body.push('}');
    body
}

/// Bound: `decode_answer` builds only the kept scalars (here the 4 MiB `stdout`, a few bytes
/// otherwise) and one property name at a time, so it never needs more than the body itself;
/// a decoder that builds the tree needs dozens of bytes per property. One body size, plus 1 MiB
/// for the pages of the allocator and of the test itself, is the bound. A control decodes a
/// flat body of 2 MiB with `decode` (the tree) and must exceed four body sizes, so that the
/// measure is known to see what it bounds.
#[test]
fn a_huge_answer_is_decoded_within_the_size_of_its_body() {
    let slack = 1024 * 1024;
    for (name, body) in [
        ("nested", nested(MAX_RESPONSE_BYTES)),
        ("flat", flat(MAX_RESPONSE_BYTES)),
    ] {
        assert!(body.len() <= MAX_RESPONSE_BYTES && body.len() > MAX_RESPONSE_BYTES - 2048);
        let added = added_by(|| {
            let answer = framing::decode_answer(body.as_bytes()).expect("an answer");
            assert_eq!(answer.kind, "result");
        });
        eprintln!(
            "{name}: body {} bytes, decode_answer adds {added} bytes",
            body.len()
        );
        assert!(added < body.len() + slack, "{name}: {added} bytes");
    }
    let control = flat(2 * 1024 * 1024);
    let added = added_by(|| {
        framing::decode(control.as_bytes()).expect("a message");
    });
    eprintln!(
        "control: body {} bytes, decode adds {added} bytes",
        control.len()
    );
    assert!(added > 4 * control.len(), "control: {added} bytes");
}
