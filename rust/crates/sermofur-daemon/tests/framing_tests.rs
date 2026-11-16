//! Port of `tests/Sermofur.Tests/FramingTests.cs`, one test per .NET case, plus the properties
//! of the decoder on arbitrary bytes.

use std::io::Cursor;

use proptest::prelude::*;
use sermofur_daemon::framing::{
    self, FrameError, IpcMessage, MAX_BODY_BYTES, MAX_RESPONSE_BYTES, kind,
};

fn read(frame: &[u8]) -> Result<Option<IpcMessage>, FrameError> {
    framing::read(&mut Cursor::new(frame), MAX_BODY_BYTES)
}

fn refusal(result: Result<Option<IpcMessage>, FrameError>) -> (String, i32, String) {
    match result {
        Err(FrameError::Refused(e)) => (e.code, e.exit_code, e.message),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn message_survives_a_round_trip() {
    let sent = IpcMessage {
        id: Some(7),
        argv: Some(
            ["claim", "add", "é -- \"quoted\"", ""]
                .map(|a| Some(a.to_owned()))
                .to_vec(),
        ),
        cwd: Some(std::env::temp_dir().display().to_string()),
        ..IpcMessage::new(kind::RUN)
    };
    let mut stream = Cursor::new(Vec::new());
    framing::write(&mut stream, &sent, MAX_BODY_BYTES).expect("writes");
    stream.set_position(0);
    let received = framing::read(&mut stream, MAX_BODY_BYTES)
        .expect("reads")
        .expect("a message");
    assert_eq!(received, sent);
    assert!(matches!(
        framing::read(&mut stream, MAX_BODY_BYTES),
        Ok(None)
    ));
}

#[test]
fn null_fields_are_not_written() {
    let frame = framing::encode(&IpcMessage::new(kind::STATUS), MAX_BODY_BYTES).unwrap();
    assert_eq!(&frame[4..], b"{\"kind\":\"status\"}");
}

#[test]
fn body_at_the_limit_is_accepted_and_one_more_byte_is_refused() {
    let padding = "a".repeat(MAX_BODY_BYTES - "{\"kind\":\"\"}".len());
    let at_limit = framing::encode(&IpcMessage::new(&padding), MAX_BODY_BYTES).unwrap();
    assert_eq!(at_limit.len(), MAX_BODY_BYTES + 4);
    assert_eq!(read(&at_limit).unwrap().unwrap().kind, padding);
    let refused = framing::encode(&IpcMessage::new(&(padding + "a")), MAX_BODY_BYTES)
        .expect_err("one byte over the limit");
    assert_eq!(
        (refused.code.as_str(), refused.exit_code),
        ("request_too_large", 1)
    );
}

#[test]
fn oversized_header_is_refused_without_reading_the_body() {
    let header = u32::try_from(MAX_BODY_BYTES + 1).unwrap().to_le_bytes();
    assert_eq!(refusal(read(&header)).0, "request_too_large");
}

#[test]
fn broken_frames_are_protocol_errors() {
    for (frame, message) in [
        (&[0u8, 0, 0, 0][..], "Empty frame."),
        (&[5, 0][..], "Truncated frame header."),
        (&[5, 0, 0, 0, b'{'][..], "Truncated frame body."),
    ] {
        assert_eq!(
            refusal(read(frame)),
            ("protocol_error".to_owned(), 3, message.to_owned())
        );
    }
}

#[test]
fn malformed_bodies_are_protocol_errors() {
    for body in ["not json", "{}", "{\"kind\":\"\"}", "{\"kind\":3}", "[]"] {
        let error = framing::decode(body.as_bytes()).expect_err(body);
        assert_eq!(error.code, "protocol_error", "{body}");
    }
}

#[test]
fn invalid_utf8_is_not_repaired() {
    let body = [&b"{\"kind\":\""[..], &[0xC3, 0x28], b"\"}"].concat();
    assert_eq!(framing::decode(&body).unwrap_err().code, "protocol_error");
}

#[test]
fn unknown_fields_are_ignored() {
    let message = framing::decode(b"{\"kind\":\"status\",\"future\":{\"x\":1}}").unwrap();
    assert_eq!(message.kind, kind::STATUS);
}

#[test]
fn a_response_limit_reads_what_a_request_limit_refuses() {
    let frame = framing::encode(
        &IpcMessage {
            stdout: Some("r".repeat(MAX_BODY_BYTES)),
            ..IpcMessage::new(kind::RESULT)
        },
        MAX_RESPONSE_BYTES,
    )
    .unwrap();
    assert_eq!(refusal(read(&frame)).0, "request_too_large");
    let back = framing::read(&mut Cursor::new(&frame), MAX_RESPONSE_BYTES).unwrap();
    assert_eq!(back.unwrap().stdout.unwrap().len(), MAX_BODY_BYTES);
}

#[test]
fn a_failing_stream_is_an_io_error() {
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken"))
        }
    }
    assert!(matches!(
        framing::read(&mut Broken, MAX_BODY_BYTES),
        Err(FrameError::Io(_))
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// `ArgumentsRoundTrip`: any argument vector within the limit comes back unchanged (a Rust
    /// string cannot hold a lone surrogate, the case the .NET property leaves out).
    #[test]
    fn arguments_round_trip(argv in prop::collection::vec(any::<String>(), 0..8)) {
        let message = IpcMessage {
            argv: Some(argv.iter().cloned().map(Some).collect()),
            ..IpcMessage::new(kind::RUN)
        };
        match framing::encode(&message, MAX_BODY_BYTES) {
            Ok(frame) => prop_assert_eq!(framing::decode(&frame[4..]).unwrap(), message),
            Err(e) => prop_assert_eq!(e.code, "request_too_large"),
        }
    }

    /// Every field, any value: what is written reads back the same.
    #[test]
    fn messages_round_trip(
        kind in ".{1,20}",
        text in proptest::option::of(any::<String>()),
        number in proptest::option::of(any::<i32>()),
        id in proptest::option::of(any::<i64>()),
    ) {
        let message = IpcMessage {
            kind,
            protocol: number,
            tool_version: text.clone(),
            cwd: text.clone(),
            pid: number,
            id,
            argv: text.clone().map(|t| vec![Some(t), None]),
            exit_code: number,
            stdout: text.clone(),
            stderr: text.clone(),
            code: text.clone(),
            message: text.clone(),
            started_at: text.clone(),
            instances_open: number,
            clients: number,
            registry: text,
        };
        let frame = framing::encode(&message, MAX_RESPONSE_BYTES).unwrap();
        // An independent reader sees the same strings as the input.
        let json: serde_json::Value = serde_json::from_slice(&frame[4..]).unwrap();
        let strings = [
            ("kind", Some(&message.kind)),
            ("toolVersion", message.tool_version.as_ref()),
            ("cwd", message.cwd.as_ref()),
            ("stdout", message.stdout.as_ref()),
            ("stderr", message.stderr.as_ref()),
            ("code", message.code.as_ref()),
            ("message", message.message.as_ref()),
            ("startedAt", message.started_at.as_ref()),
            ("registry", message.registry.as_ref()),
        ];
        for (name, value) in strings {
            prop_assert_eq!(json.get(name).and_then(|v| v.as_str()), value.map(String::as_str), "{}", name);
        }
        let argv: Option<Vec<Option<&str>>> = json
            .get("argv")
            .map(|a| a.as_array().unwrap().iter().map(|v| v.as_str()).collect());
        let expected: Option<Vec<Option<&str>>> = message
            .argv
            .as_ref()
            .map(|a| a.iter().map(Option::as_deref).collect());
        prop_assert_eq!(argv, expected);
        prop_assert_eq!(read_any(&frame), Some(message));
    }

    /// A frame read in pieces of any size, with `Interrupted` at any point, gives the same
    /// message and consumes the same bytes as when it is read at once.
    #[test]
    fn partial_reads_give_the_same_message(
        kind in ".{1,20}",
        stdout in proptest::option::of(any::<String>()),
        tail in prop::collection::vec(any::<u8>(), 0..8),
        chunks in prop::collection::vec(1usize..8, 0..64),
        interrupts in prop::collection::vec(any::<bool>(), 0..64),
    ) {
        let message = IpcMessage { stdout, ..IpcMessage::new(&kind) };
        let mut input = framing::encode(&message, MAX_RESPONSE_BYTES).unwrap();
        input.extend(&tail);
        let mut whole = Cursor::new(input.clone());
        let expected = framing::read(&mut whole, MAX_RESPONSE_BYTES).unwrap();
        let mut pieces = Pieces { data: Cursor::new(input), chunks, interrupts, step: 0 };
        let got = framing::read(&mut pieces, MAX_RESPONSE_BYTES).unwrap();
        prop_assert_eq!(&got, &expected);
        prop_assert_eq!(got, Some(message));
        prop_assert_eq!(pieces.data.position(), whole.position());
    }
}

/// A stream that serves its bytes in the given chunk sizes (then 1 byte at a time), and fails
/// with `Interrupted` before a chunk when the matching flag is set.
struct Pieces {
    data: Cursor<Vec<u8>>,
    chunks: Vec<usize>,
    interrupts: Vec<bool>,
    step: usize,
}

impl std::io::Read for Pieces {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let step = self.step;
        self.step += 1;
        if self.interrupts.get(step).copied().unwrap_or(false) {
            self.interrupts[step] = false;
            self.step = step;
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let size = self
            .chunks
            .get(step)
            .copied()
            .unwrap_or(1)
            .min(buffer.len());
        std::io::Read::read(&mut self.data, &mut buffer[..size])
    }
}

fn read_any(frame: &[u8]) -> Option<IpcMessage> {
    framing::read(&mut Cursor::new(frame), MAX_RESPONSE_BYTES).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// `ArbitraryBytesAreAMessageOrARefusal`: arbitrary bytes never escape as anything but a
    /// message or a known refusal, and never panic.
    #[test]
    fn arbitrary_bytes_are_a_message_or_a_refusal(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
        match read(&bytes) {
            Ok(_) => {}
            Err(FrameError::Refused(e)) => {
                prop_assert!(e.code == "protocol_error" || e.code == "request_too_large");
            }
            Err(FrameError::Io(e)) => prop_assert!(false, "unexpected I/O error {e}"),
        }
    }

    /// The same for an arbitrary body behind a valid header, so that the JSON reader is reached.
    #[test]
    fn arbitrary_bodies_are_a_message_or_a_protocol_error(
        body in prop_oneof![
            prop::collection::vec(any::<u8>(), 1..256),
            r#"[{}\[\]":, a-z0-9\\.-]{1,64}"#.prop_map(String::into_bytes),
        ]
    ) {
        if let Err(e) = framing::decode(&body) {
            prop_assert_eq!(e.code, "protocol_error");
        }
    }

    /// The same through `read`, behind a header that may be off by up to two bytes and before
    /// arbitrary trailing bytes: a message consumes exactly its header and its body. Half of the
    /// bodies are a valid message (`framing::encode` of an arbitrary kind), so that the
    /// consumption check runs on a known share of cases (every exact header among them).
    #[test]
    fn framed_arbitrary_bodies_are_a_message_or_a_refusal(
        body in prop_oneof![
            prop::collection::vec(any::<u8>(), 1..256),
            ".{1,32}".prop_map(|kind| {
                framing::encode(&IpcMessage::new(&kind), MAX_BODY_BYTES).expect("a valid message")
                    [4..]
                    .to_vec()
            }),
        ],
        delta in -2i64..=2,
        tail in prop::collection::vec(any::<u8>(), 0..8),
    ) {
        let length = u32::try_from((i64::try_from(body.len()).unwrap() + delta).max(0)).unwrap();
        let mut stream = Cursor::new([&length.to_le_bytes()[..], &body, &tail].concat());
        match framing::read(&mut stream, MAX_BODY_BYTES) {
            Ok(Some(_)) => prop_assert_eq!(stream.position(), 4 + u64::from(length)),
            Ok(None) => prop_assert!(false, "a header was there"),
            Err(FrameError::Refused(e)) => {
                prop_assert!(e.code == "protocol_error" || e.code == "request_too_large");
            }
            Err(FrameError::Io(e)) => prop_assert!(false, "unexpected I/O error {e}"),
        }
    }
}

/// What [`framing::decode`] gives, as [`framing::decode_answer`] gives it: `argv` checked but
/// kept empty.
fn decoded(body: &[u8]) -> Result<IpcMessage, (String, i32, String)> {
    framing::decode(body)
        .map(|mut message| {
            message.argv = message.argv.map(|_| Vec::new());
            message
        })
        .map_err(|e| (e.code, e.exit_code, e.message))
}

fn decoded_answer(body: &[u8]) -> Result<IpcMessage, (String, i32, String)> {
    framing::decode_answer(body).map_err(|e| (e.code, e.exit_code, e.message))
}

/// Lot 7.5: an answer of almost 16 MiB whose unknown properties are an array and an object,
/// huge and nested to the limit of 64, is decoded by `decode_answer` (and `read_answer`) with
/// the fields `decode` gives, without building the containers.
#[test]
fn a_huge_answer_is_decoded_without_its_unknown_containers() {
    let item = r#"{"a":[1,-2.5e3,"xé😀",null,true,false,{}],"b":[]},"#;
    let mut body = String::with_capacity(MAX_RESPONSE_BYTES);
    body.push_str(r#"{"kind":"result","id":7,"exitCode":0,"stdout":""#);
    body.push_str(&"o".repeat(4 * 1024 * 1024));
    body.push_str(r#"","stderr":"warn\n","deep":"#);
    // The top-level object is depth 1; with 60 arrays, the `{}` of each item is at 64, the
    // limit.
    body.push_str(&"[".repeat(60));
    let room = MAX_RESPONSE_BYTES - body.len() - 4 * 1024 * 1024 - 1024;
    for _ in 0..room / item.len() {
        body.push_str(item);
    }
    body.push('0');
    body.push_str(&"]".repeat(60));
    body.push_str(r#","other":{"#);
    let mut object = 0;
    while body.len() < MAX_RESPONSE_BYTES - 1024 {
        body.push_str(&format!(r#""k{object}":{item}"#));
        object += 1;
    }
    body.push_str(r#""last":[[[["x"]]]]},"code":null}"#);
    assert!(body.len() <= MAX_RESPONSE_BYTES, "{}", body.len());
    assert!(body.len() > MAX_RESPONSE_BYTES - 2048);
    let answer = decoded_answer(body.as_bytes()).expect("an answer");
    assert_eq!(answer, decoded(body.as_bytes()).expect("a message"));
    assert_eq!(
        (answer.kind.as_str(), answer.id, answer.exit_code),
        ("result", Some(7), Some(0))
    );
    assert_eq!(
        answer.stdout.as_ref().map(String::len),
        Some(4 * 1024 * 1024)
    );
    assert_eq!(answer.stderr.as_deref(), Some("warn\n"));
    let length = u32::try_from(body.len()).unwrap().to_le_bytes();
    let frame = [&length[..], body.as_bytes()].concat();
    let read = framing::read_answer(&mut Cursor::new(frame), MAX_RESPONSE_BYTES)
        .expect("read")
        .expect("a message");
    assert_eq!(read, answer);
}

/// Lot 7.5: `decode_answer` refuses what `decode` refuses, with the same error, and accepts
/// what it accepts with the same fields.
#[test]
fn the_answer_decoder_refuses_what_the_decoder_refuses() {
    let deep = |levels: usize| {
        format!(
            r#"{{"kind":"result","x":{}1{}}}"#,
            "[".repeat(levels),
            "]".repeat(levels)
        )
    };
    let deep_objects = |levels: usize| {
        format!(
            r#"{{"kind":"result","x":{}1{}}}"#,
            r#"{"a":"#.repeat(levels),
            "}".repeat(levels)
        )
    };
    let refused = [
        deep(64),
        deep_objects(64),
        r#"{"kind":"result","x":[1,2,]}"#.to_owned(),
        r#"{"kind":"result","x":[1 2]}"#.to_owned(),
        r#"{"kind":"result","x":{"a" 1}}"#.to_owned(),
        r#"{"kind":"result","x":{"a":1,}}"#.to_owned(),
        r#"{"kind":"result","x":{1:1}}"#.to_owned(),
        r#"{"kind":"result","x":[01]}"#.to_owned(),
        r#"{"kind":"result","x":[1.]}"#.to_owned(),
        r#"{"kind":"result","x":[tru]}"#.to_owned(),
        r#"{"kind":"result","x":["\x"]}"#.to_owned(),
        "{\"kind\":\"result\",\"x\":[\"a\u{1}\"]}".to_owned(),
        r#"{"kind":"result","x":["\u12"]}"#.to_owned(),
        r#"{"kind":"result","x":[1]"#.to_owned(),
        r#"{"kind":"result","x":[1]}}"#.to_owned(),
        r#"{"kind":"result","\ud800":1}"#.to_owned(),
        r#"{"kind":"result","\udc00x":[1]}"#.to_owned(),
        r#"{"kind":"run","argv":["\ud800"]}"#.to_owned(),
        r#"{"kind":"run","argv":[1]}"#.to_owned(),
        r#"{"kind":"run","argv":[["a"]]}"#.to_owned(),
        r#"{"kind":"result","stdout":["a"]}"#.to_owned(),
        r#"{"kind":"result","stdout":{}}"#.to_owned(),
        r#"{"kind":"result","id":[1]}"#.to_owned(),
        r#"{"kind":"result","exitCode":{}}"#.to_owned(),
        r#"{"kind":["result"]}"#.to_owned(),
        "null".to_owned(),
        " null ".to_owned(),
        r#"{"id":1,"x":[[]]}"#.to_owned(),
        r#"{"kind":null,"x":{}}"#.to_owned(),
        r#"{"kind":"","x":{}}"#.to_owned(),
        "[]".to_owned(),
        "{}".to_owned(),
    ];
    for body in &refused {
        let expected = decoded(body.as_bytes());
        assert!(expected.is_err(), "{body}");
        assert_eq!(decoded_answer(body.as_bytes()), expected, "{body}");
    }
    let accepted = [
        deep(63),
        deep_objects(63),
        r#"{"kind":"result","x":{"\ud800":[1]}}"#.to_owned(),
        r#"{"kind":"result","x":["\ud800",{"\udc00":"\ud800"}]}"#.to_owned(),
        r#"{"kind":"run","argv":["a",null,"é"]}"#.to_owned(),
        r#"{"kind":"run","argv":[]}"#.to_owned(),
        r#"{"kind":"run","argv":null}"#.to_owned(),
        r#"{"kind":"a","kind":"result","stdout":"x","stdout":null}"#.to_owned(),
        r#" { "kind" : "result" , "x" : [ ] , "y" : { } } "#.to_owned(),
    ];
    for body in &accepted {
        let expected = decoded(body.as_bytes());
        assert!(expected.is_ok(), "{body}: {expected:?}");
        assert_eq!(decoded_answer(body.as_bytes()), expected, "{body}");
    }
}

/// Any JSON value, nested a few levels.
fn json_value() -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::from),
        any::<i64>().prop_map(serde_json::Value::from),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(serde_json::Value::from),
        any::<String>().prop_map(serde_json::Value::from),
    ];
    leaf.prop_recursive(4, 32, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(serde_json::Value::from),
            prop::collection::btree_map(".{0,6}", inner, 0..6)
                .prop_map(|map| serde_json::Value::Object(map.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Lot 7.5: an encoded message, with an unknown property of any JSON value placed first,
    /// decodes the same through both decoders.
    #[test]
    fn both_decoders_agree_on_encoded_messages(
        kind in ".{0,12}",
        text in proptest::option::of(any::<String>()),
        number in proptest::option::of(any::<i32>()),
        argv in proptest::option::of(prop::collection::vec(proptest::option::of(".{0,8}"), 0..4)),
        unknown in proptest::option::of(json_value()),
    ) {
        let message = IpcMessage {
            id: number.map(i64::from),
            argv,
            exit_code: number,
            stdout: text.clone(),
            stderr: text.clone(),
            code: text.clone(),
            message: text,
            ..IpcMessage::new(&kind)
        };
        let frame = framing::encode(&message, MAX_RESPONSE_BYTES).unwrap();
        let mut body = frame[4..].to_vec();
        if let Some(unknown) = unknown {
            let property = format!("\"zz\":{},", serde_json::to_string(&unknown).unwrap());
            body.splice(1..1, property.into_bytes());
        }
        prop_assert_eq!(decoded_answer(&body), decoded(&body));
    }

    /// Lot 7.5: bodies made of JSON punctuation agree too, refusals included.
    #[test]
    fn both_decoders_agree_on_arbitrary_bodies(
        body in r#"\{"kind":"r",[{}\[\]":, a-z0-9\\.-]{0,48}"#,
    ) {
        prop_assert_eq!(decoded_answer(body.as_bytes()), decoded(body.as_bytes()));
    }

    /// Lot 7.5: bodies built token by token, with lone `\uD8xx` and `\uDCxx` escapes in names,
    /// strings and `argv`, containers nested 58 to 66 deep, known names in any order and
    /// repeated, and an unknown property inserted at a random position.
    #[test]
    fn both_decoders_agree_on_built_bodies(
        properties in prop::collection::vec((name_token(), property_value()), 0..6),
        unknown in (string_token(), property_value()),
        at in any::<prop::sample::Index>(),
        with_kind in any::<bool>(),
    ) {
        let mut properties: Vec<String> = properties
            .into_iter()
            .map(|(name, value)| format!("{name}:{value}"))
            .collect();
        if with_kind {
            // A share of bodies that both decoders accept.
            properties.insert(0, r#""kind":"result""#.to_owned());
        }
        let position = at.index(properties.len() + 1);
        properties.insert(position, format!("{}:{}", unknown.0, unknown.1));
        let body = format!("{{{}}}", properties.join(","));
        prop_assert_eq!(decoded_answer(body.as_bytes()), decoded(body.as_bytes()), "{}", body);
    }
}

/// A JSON string token: ordinary characters, and lone high or low surrogate escapes.
fn string_token() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        4 => any::<char>().prop_map(|c| {
            let quoted = serde_json::to_string(&c.to_string()).unwrap();
            quoted[1..quoted.len() - 1].to_owned()
        }),
        1 => "[89abAB][0-9a-fA-F]{2}".prop_map(|tail| format!(r"\uD{tail}")),
        1 => "[c-fC-F][0-9a-fA-F]{2}".prop_map(|tail| format!(r"\uD{tail}")),
    ];
    prop::collection::vec(piece, 0..5).prop_map(|pieces| format!("\"{}\"", pieces.concat()))
}

/// A property name: a name the message reads, or any string token.
fn name_token() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(vec![
            "kind", "id", "argv", "exitCode", "stdout", "stderr", "code", "message", "protocol",
        ])
        .prop_map(|name| format!("\"{name}\"")),
        string_token(),
    ]
}

/// A JSON value token, nested a few levels, with string tokens everywhere.
fn value_token() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        Just("null".to_owned()),
        Just("true".to_owned()),
        Just("\"result\"".to_owned()),
        any::<i64>().prop_map(|n| n.to_string()),
        string_token(),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4)
                .prop_map(|items| format!("[{}]", items.join(","))),
            prop::collection::vec((string_token(), inner), 0..4).prop_map(|pairs| {
                let pairs: Vec<String> =
                    pairs.into_iter().map(|(k, v)| format!("{k}:{v}")).collect();
                format!("{{{}}}", pairs.join(","))
            }),
        ]
    })
}

/// The value of a property: any token, an `argv`-like array of strings, or a container nested
/// 57 to 65 levels under the top-level object (58 to 66 in all).
fn property_value() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => value_token(),
        1 => prop::collection::vec(prop_oneof![string_token(), Just("null".to_owned())], 0..4)
            .prop_map(|items| format!("[{}]", items.join(","))),
        1 => (57usize..=65, any::<bool>()).prop_map(|(levels, objects)| if objects {
            format!("{}1{}", r#"{"a":"#.repeat(levels), "}".repeat(levels))
        } else {
            format!("{}1{}", "[".repeat(levels), "]".repeat(levels))
        }),
    ]
}
