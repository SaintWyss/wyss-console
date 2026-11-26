use super::{Manifest, short};
use std::collections::BTreeSet;

pub(super) fn parse(file: &str, text: &str) -> Manifest {
    let mut result = match file {
        "Cargo.toml" => cargo(text),
        "package.json" => node(text),
        "pyproject.toml" => python(text),
        "go.mod" => go(text),
        "pom.xml" => maven(text),
        "build.gradle" | "build.gradle.kts" => gradle(text),
        "Makefile" => make(text),
        _ => Manifest::default(),
    };
    result.file = file.into();
    result
}

fn cargo(text: &str) -> Manifest {
    let Ok(t) = text.parse::<toml::Table>() else {
        return Manifest::default();
    };
    let p = t
        .get("package")
        .and_then(|p| p.as_table())
        .or_else(|| t.get("workspace")?.get("package")?.as_table());
    let field = |key| {
        p.and_then(|p| p.get(key))
            .and_then(toml::Value::as_str)
            .map(short)
    };
    let mut m = Manifest {
        package: field("name"),
        version: field("version"),
        license: field("license"),
        ..Manifest::default()
    };
    for key in ["edition", "rust-version"] {
        if let Some(v) = field(key) {
            m.edition.push((key.into(), v));
        }
    }
    let mut deps = BTreeSet::new();
    let work = t.get("workspace").and_then(|v| v.get("dependencies"));
    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
        for (name, value) in t
            .get(key)
            .and_then(toml::Value::as_table)
            .into_iter()
            .flatten()
        {
            if !deps.insert(name) {
                continue;
            }
            let inherited = (value.get("workspace").and_then(toml::Value::as_bool) == Some(true))
                .then(|| work.and_then(|w| w.get(name)))
                .flatten()
                .unwrap_or(value);
            let version = inherited
                .as_str()
                .or_else(|| inherited.get("version").and_then(toml::Value::as_str));
            let actual = inherited
                .get("package")
                .and_then(toml::Value::as_str)
                .unwrap_or(name);
            m.dependencies
                .push((short(actual), version.map(short).unwrap_or_default()));
        }
    }
    for bin in t
        .get("bin")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = bin.get("name").and_then(toml::Value::as_str) {
            m.runs
                .push(("Cargo".into(), format!("cargo run --bin {}", short(name))));
        }
    }
    m
}

#[derive(Default)]
struct OrderedPairs(Vec<(String, String)>);

impl<'de> serde::Deserialize<'de> for OrderedPairs {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct Pairs;
        impl<'de> serde::de::Visitor<'de> for Pairs {
            type Value = OrderedPairs;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a map of names and versions")
            }

            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut out = Vec::new();
                while let Some((name, value)) = map.next_entry::<String, serde_json::Value>()? {
                    if let Some(value) = value.as_str() {
                        out.push((short(&name), short(value)));
                    }
                }
                Ok(OrderedPairs(out))
            }
        }
        de.deserialize_map(Pairs)
    }
}

fn node(text: &str) -> Manifest {
    #[derive(Default, serde::Deserialize)]
    #[serde(default, rename_all = "camelCase")]
    struct Package {
        name: Option<String>,
        version: Option<String>,
        license: Option<serde_json::Value>,
        engines: OrderedPairs,
        scripts: OrderedPairs,
        dependencies: OrderedPairs,
        dev_dependencies: OrderedPairs,
        peer_dependencies: OrderedPairs,
        optional_dependencies: OrderedPairs,
        eslint_config: Option<serde_json::Value>,
        prettier: Option<serde_json::Value>,
        jest: Option<serde_json::Value>,
        vitest: Option<serde_json::Value>,
    }
    let Ok(p) = serde_json::from_str::<Package>(text) else {
        return Manifest::default();
    };
    let mut m = Manifest {
        package: p.name.as_deref().map(short),
        version: p.version.as_deref().map(short),
        license: p
            .license
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .map(short),
        edition: p.engines.0.into_iter().take(20).collect(),
        ..Manifest::default()
    };
    let mut seen = BTreeSet::new();
    for deps in [
        p.dependencies,
        p.dev_dependencies,
        p.peer_dependencies,
        p.optional_dependencies,
    ] {
        m.dependencies.extend(
            deps.0
                .into_iter()
                .filter(|(name, _)| seen.insert(name.clone())),
        );
    }
    m.runs.extend(
        p.scripts
            .0
            .into_iter()
            .map(|(name, value)| (format!("pnpm run {name}"), value)),
    );
    m.quality.extend(
        [
            ("eslint", p.eslint_config),
            ("prettier", p.prettier),
            ("jest", p.jest),
            ("vitest", p.vitest),
        ]
        .into_iter()
        .filter(|(_, config)| config.is_some())
        .map(|(name, _)| name.into()),
    );
    m
}

fn python(text: &str) -> Manifest {
    let Ok(t) = text.parse::<toml::Table>() else {
        return Manifest::default();
    };
    let project = t.get("project");
    let poetry = t.get("tool").and_then(|v| v.get("poetry"));
    let field = |key| {
        project
            .and_then(|p| p.get(key))
            .or_else(|| poetry.and_then(|p| p.get(key)))
    };
    let str_field = |key| field(key).and_then(toml::Value::as_str).map(short);
    let mut m = Manifest {
        package: str_field("name"),
        version: str_field("version"),
        license: str_field("license")
            .or_else(|| field("license")?.get("text")?.as_str().map(short)),
        ..Manifest::default()
    };
    if let Some(p) = str_field("requires-python").or_else(|| {
        poetry?
            .get("dependencies")?
            .get("python")?
            .as_str()
            .map(short)
    }) {
        m.edition.push(("Python".into(), p));
    }
    for value in project
        .and_then(|p| p.get("dependencies"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
    {
        let end = value
            .find(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_' | '.'))
            .unwrap_or(value.len());
        m.dependencies
            .push((short(&value[..end]), short(value[end..].trim())));
    }
    for (name, value) in poetry
        .and_then(|p| p.get("dependencies"))
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
    {
        if name != "python" {
            let version = value
                .as_str()
                .or_else(|| value.get("version").and_then(toml::Value::as_str));
            m.dependencies
                .push((short(name), version.map(short).unwrap_or_default()));
        }
    }
    m.quality.extend(
        ["ruff", "pytest", "black"]
            .into_iter()
            .filter(|key| t.get("tool").and_then(|v| v.get(*key)).is_some())
            .map(str::to_owned),
    );
    m
}

fn go(text: &str) -> Manifest {
    let mut m = Manifest::default();
    let mut in_requires = false;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        match (words.next(), words.next()) {
            (Some("module"), Some(module)) => m.package = Some(short(module.trim_matches('"'))),
            (Some(key @ ("go" | "toolchain")), Some(version)) => {
                m.edition.push((key.into(), short(version)))
            }
            (Some("require"), Some("(")) => in_requires = true,
            (Some(")"), _) => in_requires = false,
            (Some("require"), Some(name)) if !line.contains("// indirect") => {
                m.dependencies
                    .push((short(name), short(words.next().unwrap_or(""))));
            }
            (Some(name), Some(version))
                if in_requires && !name.starts_with("//") && !line.contains("// indirect") =>
            {
                m.dependencies.push((short(name), short(version)));
            }
            _ => {}
        }
    }
    m
}

// The Maven fields are text nodes. Never resolve DTDs, entities, parents or property references.
fn maven(text: &str) -> Manifest {
    let mut m = Manifest::default();
    let mut ancestry = Vec::new();
    let mut dep = (String::new(), String::new(), String::new());
    let mut rest = text;
    while let Some((_, after)) = rest.split_once('<') {
        if let Some(comment) = after.strip_prefix("!--") {
            let Some((_, next)) = comment.split_once("-->") else {
                break;
            };
            rest = next;
            continue;
        }
        let Some((raw, next)) = after.split_once('>') else {
            break;
        };
        rest = next;
        let tag = raw.split_whitespace().next().unwrap_or("");
        if let Some(tag) = tag.strip_prefix('/') {
            if tag == "dependency"
                && ancestry.as_slice() == ["project", "dependencies", "dependency"]
                && !dep.1.is_empty()
            {
                let (group, name, version) = std::mem::take(&mut dep);
                m.dependencies.push((
                    if group.is_empty() {
                        name
                    } else {
                        format!("{group}:{name}")
                    },
                    version,
                ));
            }
            if ancestry.pop() != Some(tag) {
                break;
            }
            continue;
        }
        if tag.starts_with(['?', '!']) || raw.ends_with('/') || ancestry.len() >= 64 {
            continue;
        }
        let value = || short(next.split('<').next().unwrap_or("").trim());
        match ancestry.as_slice() {
            ["project"] if tag == "artifactId" => m.package = Some(value()),
            ["project"] if tag == "version" => m.version = Some(value()),
            ["project", "properties"]
                if matches!(
                    tag,
                    "java.version" | "maven.compiler.release" | "maven.compiler.source"
                ) =>
            {
                m.edition.push((tag.into(), value()));
            }
            ["project", "dependencies"] if tag == "dependency" => dep = Default::default(),
            ["project", "dependencies", "dependency"] => match tag {
                "groupId" => dep.0 = value(),
                "artifactId" => dep.1 = value(),
                "version" => dep.2 = value(),
                _ => {}
            },
            ["project", "licenses", "license"] if tag == "name" => m.license = Some(value()),
            _ => {}
        }
        ancestry.push(tag);
    }
    m
}

fn gradle(text: &str) -> Manifest {
    let mut m = Manifest::default();
    for line in text.lines().map(str::trim) {
        if line.starts_with("//") {
            continue;
        }
        let key = line
            .split(|c: char| c.is_whitespace() || matches!(c, '(' | '='))
            .next()
            .unwrap_or("");
        let rhs = line[key.len()..]
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '(' | '='));
        let quoted = rhs
            .chars()
            .next()
            .filter(|c| matches!(c, '\'' | '"'))
            .and_then(|quote| rhs[1..].split_once(quote).map(|(v, _)| v));
        match key {
            "version" => m.version = quoted.map(short),
            "rootProject.name" => m.package = quoted.map(short),
            "sourceCompatibility" | "targetCompatibility" | "languageVersion" | "jvmToolchain" => {
                m.edition.push((
                    key.into(),
                    short(quoted.unwrap_or(rhs.trim_end_matches(')'))),
                ));
            }
            "implementation" | "api" | "compileOnly" | "runtimeOnly" | "testImplementation"
            | "classpath" => {
                if let Some(coord) = quoted {
                    let mut parts = coord.splitn(3, ':');
                    if let (Some(group), Some(name)) = (parts.next(), parts.next()) {
                        m.dependencies.push((
                            short(&format!("{group}:{name}")),
                            short(parts.next().unwrap_or("")),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    m
}

fn make(text: &str) -> Manifest {
    let mut m = Manifest::default();
    let mut seen = BTreeSet::new();
    for line in text.lines() {
        if line.starts_with(['\t', '#', ' ']) {
            continue;
        }
        if let Some((lhs, rhs)) = line.split_once(':')
            && !rhs.starts_with('=')
            && !lhs.contains(['=', '%', '$'])
        {
            for target in lhs.split_whitespace().filter(|t| !t.starts_with('.')) {
                if seen.insert(target) {
                    m.runs
                        .push(("Make".into(), format!("make {}", short(target))));
                    if m.runs.len() == 20 {
                        return m;
                    }
                }
            }
        }
    }
    m
}

pub(super) fn frameworks(manifests: &[Manifest]) -> Vec<&'static str> {
    let names = [
        "gtk4",
        "tauri",
        "svelte",
        "react",
        "next",
        "django",
        "fastapi",
        "spring",
        "vue",
        "angular",
        "express",
        "flask",
        "axum",
        "actix-web",
        "gin",
        "laravel",
        "rails",
    ];
    names
        .into_iter()
        .filter(|name| {
            manifests.iter().any(|m| {
                m.dependencies.iter().any(|(key, _)| {
                    let key = key.to_ascii_lowercase();
                    let component = key.rsplit(['/', ':']).next().unwrap_or("");
                    component == *name
                        || component.starts_with(&format!("{name}-"))
                        || key.starts_with(&format!("@{name}/"))
                })
            })
        })
        .collect()
}

pub(super) fn workflow(file: &str, text: &str) -> (String, String) {
    let mut name = None;
    let mut triggers = Vec::new();
    let mut indent = None;
    let mut reading = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let spaces = raw.len() - raw.trim_start().len();
        if spaces == 0 {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.split(" #").next().unwrap_or("").trim();
            reading = matches!(key.trim_matches(['\'', '"']), "on");
            if key == "name" {
                name = Some(short(value.trim_matches(['\'', '"'])));
            }
            if reading && !value.is_empty() {
                let value = value.trim_matches(['[', ']', '{', '}']);
                triggers.extend(
                    value
                        .split(',')
                        .take(20)
                        .map(|v| short(v.trim().trim_matches(['\'', '"']))),
                );
                reading = false;
            }
        } else if reading && spaces == *indent.get_or_insert(spaces) && triggers.len() < 20 {
            let item = line
                .strip_prefix("- ")
                .or_else(|| line.split_once(':').map(|(k, _)| k));
            if let Some(item) = item {
                triggers.push(short(item.trim_matches(['\'', '"'])));
            }
        }
    }
    (name.unwrap_or_else(|| short(file)), triggers.join(", "))
}
