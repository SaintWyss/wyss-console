//! The TOML scanner: what it places, and that it refuses what it cannot place.
use super::tomlspan::parse;

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
fn it_places_tables_across_multiline_values() {
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
    let one = &doc.tables[4];
    let next = &doc.tables[5];
    assert!(SAMPLE[one.header.0..one.body_end].ends_with("[not.a.header]\n\"\"\"\n"));
    assert!(one.body_end <= next.header.0);
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
