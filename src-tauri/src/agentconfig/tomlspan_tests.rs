//! The TOML scanner: what it places, and that it refuses what it cannot place.
use super::tomlspan::{insert_statement, parse, remove_statement, replace_value};

const SAMPLE: &str = r#"# top comment
model = "gpt-6"   # trailing
model_reasoning_effort = 'high'

[features]
hooks = true

[[skills.config]]
path = "/a/b/SKILL.md"   # the first
enabled = false

[[skills.config]]
path = "/c/d"

[mcp_servers.one]
command = "one"
args = [
  "-y",   # inline comment
  "x]",
]
env = { A = "1", B = "2" }
text = """
multi # not a comment
[not.a.header]
"""

[mcp_servers."two words"]
command = 'two'
"#;

#[test]
fn it_places_tables_entries_and_their_values() {
    let doc = parse(SAMPLE).unwrap();
    let paths: Vec<_> = doc.tables.iter().map(|t| t.path.join(".")).collect();
    assert_eq!(
        paths,
        [
            "",
            "features",
            "skills.config",
            "skills.config",
            "mcp_servers.one",
            "mcp_servers.two words"
        ]
    );
    assert!(doc.tables[2].array && doc.tables[3].array && !doc.tables[1].array);
    let root = &doc.tables[0];
    let model = root.entry(&["model".to_owned()]).unwrap();
    assert_eq!(&SAMPLE[model.value.0..model.value.1], "\"gpt-6\"");
    // A trailing comment is not part of the value.
    let effort = root.entry(&["model_reasoning_effort".to_owned()]).unwrap();
    assert_eq!(&SAMPLE[effort.value.0..effort.value.1], "'high'");
    let one = &doc.tables[4];
    let args = one
        .entry(&["mcp_servers".into(), "one".into(), "args".into()])
        .unwrap();
    assert!(
        SAMPLE[args.value.0..args.value.1].starts_with('[')
            && SAMPLE[args.value.0..args.value.1].ends_with(']')
    );
    let text = one
        .entry(&["mcp_servers".into(), "one".into(), "text".into()])
        .unwrap();
    assert!(SAMPLE[text.value.0..text.value.1].contains("[not.a.header]"));
    assert_eq!(doc.tables_at(&["mcp_servers", "two words"]).len(), 1);
}

#[test]
fn a_statement_is_removed_with_its_line_and_nothing_else() {
    let doc = parse(SAMPLE).unwrap();
    let enabled = doc.tables[2]
        .entry(&["skills".into(), "config".into(), "enabled".into()])
        .unwrap();
    let after = remove_statement(enabled).apply(SAMPLE.as_bytes());
    assert_eq!(
        String::from_utf8(after).unwrap(),
        SAMPLE.replace("enabled = false\n", "")
    );
}

#[test]
fn a_value_is_replaced_in_place_and_its_comment_stays() {
    let doc = parse(SAMPLE).unwrap();
    let hooks = doc.tables[1]
        .entry(&["features".into(), "hooks".into()])
        .unwrap();
    let after = replace_value(hooks, "false").apply(SAMPLE.as_bytes());
    assert_eq!(
        String::from_utf8(after).unwrap(),
        SAMPLE.replace("hooks = true", "hooks = false")
    );
}

#[test]
fn a_statement_is_added_after_the_last_of_its_table_and_before_the_next_header() {
    let doc = parse(SAMPLE).unwrap();
    let second = &doc.tables[3];
    let after = insert_statement(SAMPLE, second, "enabled", "false").apply(SAMPLE.as_bytes());
    assert_eq!(
        String::from_utf8(after).unwrap(),
        SAMPLE.replace("path = \"/c/d\"\n", "path = \"/c/d\"\nenabled = false\n")
    );
    // A file that does not end with a newline still gets the statement on its own line.
    let bare = "[a]\nk = 1";
    let doc = parse(bare).unwrap();
    let after = insert_statement(bare, &doc.tables[1], "enabled", "true").apply(bare.as_bytes());
    assert_eq!(
        String::from_utf8(after).unwrap(),
        "[a]\nk = 1\nenabled = true\n"
    );
    // CRLF files keep their line endings.
    let crlf = "[a]\r\nk = 1\r\n";
    let doc = parse(crlf).unwrap();
    let after = insert_statement(crlf, &doc.tables[1], "enabled", "true").apply(crlf.as_bytes());
    assert_eq!(
        String::from_utf8(after).unwrap(),
        "[a]\r\nk = 1\r\nenabled = true\r\n"
    );
}

#[test]
fn what_it_cannot_place_with_certainty_is_refused() {
    for text in [
        "a = [1, 2\n",              // an array never closed
        "a = \"open\n",             // a string that runs off its line
        "[a\nk = 1\n",              // a header never closed
        "= 1\n",                    // no key
        "a = \"\"\"never closed\n", // a multi-line string never closed
    ] {
        assert!(parse(text).is_err(), "{text:?}");
    }
}

#[test]
fn it_reads_whatever_the_real_parser_reads_of_a_corpus_of_odd_but_valid_files() {
    for text in [
        "",
        "\n\n# only comments\n",
        "a.b.c = 1\n[t]\nx.y = 2\n",
        "[ spaced . header ]\nk=1\n",
        "k = 'literal \\ backslash'\n",
        "k = \"esc \\\" quote # not a comment\"\n",
        "k = [ [1,2], [3,4] ]\n",
        "k = { a = { b = 1 } }\n",
        "k = 1979-05-27T07:32:00Z\n",
        "k = +1_000\n",
        "t = '''\nmulti\n'''\n",
        "[[a]]\n[[a]]\nx = 1\n",
    ] {
        toml::from_str::<toml::Value>(text)
            .unwrap_or_else(|e| panic!("fixture invalid {text:?}: {e}"));
        parse(text).unwrap_or_else(|e| panic!("refused a valid file {text:?}: {e:?}"));
    }
}
