//! What a project is made of, read from its folder: the technologies its manifests name, the
//! languages of its files, the architecture its layout suggests, its README, and the types its
//! source declares (for UML). Read-only except for the README, which only `save_readme` writes.
//!
//! Every answer carries names relative to the project's folder; the folder's path stays here.
//! The walk is bounded (files, depth, bytes) so a huge folder answers quickly and partially.
use super::command::{ErrorCode, Failure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 20_000;
const MAX_DEPTH: usize = 12;
const MAX_SOURCE_BYTES: u64 = 256 * 1024;
const MAX_SOURCE_FILES: usize = 1_500;
const MAX_TYPES: usize = 400;
const MAX_MEMBERS: usize = 24;
pub const MAX_README_BYTES: usize = 512 * 1024;

/// Folders that are tooling, dependencies or output, never the project's own structure.
const SKIPPED: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    "vendor",
    ".venv",
    "venv",
    "__pycache__",
    ".gradle",
    ".idea",
    ".vscode",
    "coverage",
    ".cache",
    "bin",
    "obj",
    "Pods",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub name: String,
    pub files: usize,
    pub folders: usize,
    pub bytes: u64,
    /// The walk stopped at a bound; counts are a lower limit.
    pub truncated: bool,
    pub languages: Vec<Language>,
    pub technologies: Vec<Technology>,
    pub architecture: Vec<Pattern>,
    pub structure: Vec<Entry>,
    pub manifests: Vec<String>,
    pub scripts: Vec<Script>,
    pub git: bool,
    pub license: Option<String>,
    pub readme: Option<Readme>,
    pub types: Vec<TypeDecl>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Language {
    pub name: String,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Technology {
    pub name: String,
    /// language, framework, library, tooling, testing, database, infrastructure, ci
    pub category: &'static str,
    pub version: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pattern {
    pub name: &'static str,
    pub description: &'static str,
    /// 0..=100
    pub confidence: u8,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub files: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub source: String,
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Readme {
    pub name: String,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeDecl {
    pub name: String,
    /// class, interface, struct, enum, trait, type
    pub kind: &'static str,
    pub file: String,
    pub language: &'static str,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub fields: Vec<String>,
    pub methods: Vec<String>,
}

fn io() -> Failure {
    Failure::new(ErrorCode::IoFailure)
}

// ---- the walk ---------------------------------------------------------------------------------

struct Walk {
    files: Vec<(String, u64)>,
    folders: usize,
    truncated: bool,
}

fn walk(root: &Path) -> Walk {
    let mut out = Walk {
        files: Vec::new(),
        folders: 0,
        truncated: false,
    };
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            // Links are not followed: the walk stays inside the folder.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if kind.is_dir() {
                if SKIPPED.contains(&name.as_str()) {
                    continue;
                }
                out.folders += 1;
                if depth + 1 < MAX_DEPTH {
                    stack.push((path, depth + 1));
                } else {
                    out.truncated = true;
                }
            } else if kind.is_file() {
                if out.files.len() >= MAX_FILES {
                    out.truncated = true;
                    return out;
                }
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                if let Ok(rel) = path.strip_prefix(root) {
                    out.files
                        .push((rel.to_string_lossy().replace('\\', "/"), size));
                }
            }
        }
    }
    out.files.sort();
    out
}

fn language_of(file: &str) -> Option<&'static str> {
    let lower = file.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    if base == "dockerfile" || base.starts_with("dockerfile.") {
        return Some("Dockerfile");
    }
    if base == "makefile" {
        return Some("Makefile");
    }
    let ext = base.rsplit_once('.').map(|(_, e)| e)?;
    Some(match ext {
        "rs" => "Rust",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TypeScript (TSX)",
        "js" | "mjs" | "cjs" => "JavaScript",
        "jsx" => "JavaScript (JSX)",
        "svelte" => "Svelte",
        "vue" => "Vue",
        "py" => "Python",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "go" => "Go",
        "rb" => "Ruby",
        "php" => "PHP",
        "cs" => "C#",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "swift" => "Swift",
        "dart" => "Dart",
        "scala" => "Scala",
        "ex" | "exs" => "Elixir",
        "lua" => "Lua",
        "sh" | "bash" | "zsh" => "Shell",
        "ps1" => "PowerShell",
        "sql" => "SQL",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "scss" | "sass" => "SCSS",
        "less" => "Less",
        "md" | "mdx" => "Markdown",
        "json" => "JSON",
        "yml" | "yaml" => "YAML",
        "toml" => "TOML",
        "xml" => "XML",
        "proto" => "Protocol Buffers",
        "graphql" | "gql" => "GraphQL",
        "tf" => "Terraform",
        _ => return None,
    })
}

// ---- technologies -----------------------------------------------------------------------------

/// Known dependencies and what they say about the project.
fn known(dep: &str) -> Option<(&'static str, &'static str)> {
    let d = dep.to_ascii_lowercase();
    let pair = match d.as_str() {
        // JavaScript / TypeScript
        "react" => ("React", "framework"),
        "next" => ("Next.js", "framework"),
        "vue" => ("Vue", "framework"),
        "nuxt" => ("Nuxt", "framework"),
        "svelte" => ("Svelte", "framework"),
        "@sveltejs/kit" => ("SvelteKit", "framework"),
        "@angular/core" => ("Angular", "framework"),
        "solid-js" => ("SolidJS", "framework"),
        "astro" => ("Astro", "framework"),
        "express" => ("Express", "framework"),
        "fastify" => ("Fastify", "framework"),
        "@nestjs/core" => ("NestJS", "framework"),
        "koa" => ("Koa", "framework"),
        "hono" => ("Hono", "framework"),
        "electron" => ("Electron", "framework"),
        "@tauri-apps/api" => ("Tauri", "framework"),
        "react-native" => ("React Native", "framework"),
        "typescript" => ("TypeScript", "language"),
        "vite" => ("Vite", "tooling"),
        "webpack" => ("Webpack", "tooling"),
        "esbuild" => ("esbuild", "tooling"),
        "eslint" => ("ESLint", "tooling"),
        "prettier" => ("Prettier", "tooling"),
        "tailwindcss" => ("Tailwind CSS", "library"),
        "redux" | "@reduxjs/toolkit" => ("Redux", "library"),
        "zustand" => ("Zustand", "library"),
        "graphql" => ("GraphQL", "library"),
        "@apollo/client" => ("Apollo Client", "library"),
        "axios" => ("Axios", "library"),
        "zod" => ("Zod", "library"),
        "jest" => ("Jest", "testing"),
        "vitest" => ("Vitest", "testing"),
        "mocha" => ("Mocha", "testing"),
        "@playwright/test" | "playwright" => ("Playwright", "testing"),
        "cypress" => ("Cypress", "testing"),
        "prisma" | "@prisma/client" => ("Prisma", "database"),
        "typeorm" => ("TypeORM", "database"),
        "sequelize" => ("Sequelize", "database"),
        "mongoose" => ("Mongoose", "database"),
        "drizzle-orm" => ("Drizzle ORM", "database"),
        "pg" => ("PostgreSQL client", "database"),
        "mysql2" => ("MySQL client", "database"),
        "redis" | "ioredis" => ("Redis", "database"),
        // Rust
        "tauri" => ("Tauri", "framework"),
        "tokio" => ("Tokio", "library"),
        "actix-web" => ("Actix Web", "framework"),
        "axum" => ("Axum", "framework"),
        "rocket" => ("Rocket", "framework"),
        "serde" => ("Serde", "library"),
        "diesel" => ("Diesel", "database"),
        "sqlx" => ("SQLx", "database"),
        "rusqlite" => ("SQLite (rusqlite)", "database"),
        "clap" => ("Clap", "library"),
        "bevy" => ("Bevy", "framework"),
        // Python
        "django" => ("Django", "framework"),
        "flask" => ("Flask", "framework"),
        "fastapi" => ("FastAPI", "framework"),
        "sqlalchemy" => ("SQLAlchemy", "database"),
        "pytest" => ("pytest", "testing"),
        "numpy" => ("NumPy", "library"),
        "pandas" => ("pandas", "library"),
        "torch" => ("PyTorch", "library"),
        "tensorflow" => ("TensorFlow", "library"),
        "pydantic" => ("Pydantic", "library"),
        // Go
        "github.com/gin-gonic/gin" => ("Gin", "framework"),
        "github.com/labstack/echo/v4" => ("Echo", "framework"),
        "github.com/gofiber/fiber/v2" => ("Fiber", "framework"),
        "gorm.io/gorm" => ("GORM", "database"),
        // PHP
        "laravel/framework" => ("Laravel", "framework"),
        "symfony/symfony" | "symfony/framework-bundle" => ("Symfony", "framework"),
        "phpunit/phpunit" => ("PHPUnit", "testing"),
        // Ruby
        "rails" => ("Ruby on Rails", "framework"),
        "sinatra" => ("Sinatra", "framework"),
        "rspec" => ("RSpec", "testing"),
        _ => return None,
    };
    Some(pair)
}

struct Techs {
    list: Vec<Technology>,
    seen: BTreeSet<String>,
    scripts: Vec<Script>,
}

impl Techs {
    fn add(&mut self, name: &str, category: &'static str, version: Option<String>, source: &str) {
        if self.seen.insert(name.to_owned()) {
            self.list.push(Technology {
                name: name.to_owned(),
                category,
                version,
                source: source.to_owned(),
            });
        }
    }

    fn dep(&mut self, dep: &str, version: Option<String>, source: &str) {
        if let Some((name, category)) = known(dep) {
            self.add(name, category, version, source);
        }
    }
}

fn read_small(root: &Path, rel: &str) -> Option<String> {
    let path = root.join(rel);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > 2 * 1024 * 1024 {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn manifests(root: &Path, files: &[(String, u64)], t: &mut Techs) -> Vec<String> {
    let mut found = Vec::new();
    for (rel, _) in files {
        let base = rel.rsplit('/').next().unwrap_or(rel);
        // Manifests three levels deep at most: a monorepo's packages, not fixtures in tests.
        if rel.matches('/').count() > 3 {
            continue;
        }
        let lower = base.to_ascii_lowercase();
        match lower.as_str() {
            "package.json" => {
                found.push(rel.clone());
                t.add("Node.js", "language", None, rel);
                let Some(text) = read_small(root, rel) else {
                    continue;
                };
                let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };
                for key in ["dependencies", "devDependencies", "peerDependencies"] {
                    if let Some(map) = json.get(key).and_then(|v| v.as_object()) {
                        for (name, v) in map {
                            t.dep(name, v.as_str().map(str::to_owned), rel);
                        }
                    }
                }
                if json.get("workspaces").is_some() {
                    t.add("npm workspaces", "tooling", None, rel);
                }
                if let Some(map) = json.get("scripts").and_then(|v| v.as_object()) {
                    for (name, v) in map.iter().take(40) {
                        t.scripts.push(Script {
                            source: rel.clone(),
                            name: name.clone(),
                            command: v.as_str().unwrap_or("").chars().take(300).collect(),
                        });
                    }
                }
            }
            "cargo.toml" => {
                found.push(rel.clone());
                t.add("Rust", "language", None, rel);
                let Some(text) = read_small(root, rel) else {
                    continue;
                };
                let Ok(doc) = toml::from_str::<toml::Value>(&text) else {
                    continue;
                };
                if doc.get("workspace").is_some() {
                    t.add("Cargo workspace", "tooling", None, rel);
                }
                for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                    if let Some(map) = doc.get(key).and_then(|v| v.as_table()) {
                        for (name, v) in map {
                            let version = match v {
                                toml::Value::String(s) => Some(s.clone()),
                                toml::Value::Table(t) => {
                                    t.get("version").and_then(|v| v.as_str()).map(str::to_owned)
                                }
                                _ => None,
                            };
                            t.dep(name, version, rel);
                        }
                    }
                }
            }
            "pyproject.toml" | "requirements.txt" | "setup.py" | "pipfile" => {
                found.push(rel.clone());
                t.add("Python", "language", None, rel);
                let Some(text) = read_small(root, rel) else {
                    continue;
                };
                for line in text.lines() {
                    let word: String = line
                        .trim()
                        .trim_start_matches(['"', '\''])
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                        .collect();
                    if !word.is_empty() {
                        t.dep(&word, None, rel);
                    }
                }
            }
            "go.mod" => {
                found.push(rel.clone());
                t.add("Go", "language", None, rel);
                let Some(text) = read_small(root, rel) else {
                    continue;
                };
                for line in text.lines() {
                    let mut parts = line
                        .trim()
                        .trim_start_matches("require ")
                        .split_whitespace();
                    if let (Some(name), Some(version)) = (parts.next(), parts.next()) {
                        t.dep(name, Some(version.to_owned()), rel);
                    }
                }
            }
            "composer.json" => {
                found.push(rel.clone());
                t.add("PHP", "language", None, rel);
                let Some(text) = read_small(root, rel) else {
                    continue;
                };
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    for key in ["require", "require-dev"] {
                        if let Some(map) = json.get(key).and_then(|v| v.as_object()) {
                            for (name, v) in map {
                                t.dep(name, v.as_str().map(str::to_owned), rel);
                            }
                        }
                    }
                }
            }
            "gemfile" => {
                found.push(rel.clone());
                t.add("Ruby", "language", None, rel);
                let Some(text) = read_small(root, rel) else {
                    continue;
                };
                for line in text.lines() {
                    if let Some(rest) = line.trim().strip_prefix("gem ") {
                        let name = rest.trim().trim_matches(['"', '\'']);
                        let name = name.split(['"', '\'']).next().unwrap_or("");
                        t.dep(name, None, rel);
                    }
                }
            }
            "pom.xml" => {
                found.push(rel.clone());
                t.add("Java", "language", None, rel);
                t.add("Maven", "tooling", None, rel);
                if read_small(root, rel).is_some_and(|s| s.contains("spring-boot")) {
                    t.add("Spring Boot", "framework", None, rel);
                }
            }
            "build.gradle" | "build.gradle.kts" => {
                found.push(rel.clone());
                t.add("Gradle", "tooling", None, rel);
                if let Some(s) = read_small(root, rel) {
                    if s.contains("com.android") {
                        t.add("Android", "framework", None, rel);
                    }
                    if s.contains("org.springframework.boot") {
                        t.add("Spring Boot", "framework", None, rel);
                    }
                    if s.contains("kotlin") {
                        t.add("Kotlin", "language", None, rel);
                    }
                }
            }
            "pubspec.yaml" => {
                found.push(rel.clone());
                t.add("Flutter / Dart", "framework", None, rel);
            }
            "tauri.conf.json" => {
                found.push(rel.clone());
                t.add("Tauri", "framework", None, rel);
            }
            "tsconfig.json" => t.add("TypeScript", "language", None, rel),
            "dockerfile" => t.add("Docker", "infrastructure", None, rel),
            "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml" => {
                t.add("Docker Compose", "infrastructure", None, rel)
            }
            "makefile" => t.add("Make", "tooling", None, rel),
            _ => {}
        }
        if lower.ends_with(".csproj") || lower.ends_with(".sln") {
            found.push(rel.clone());
            t.add(".NET", "framework", None, rel);
        }
        if lower.ends_with(".tf") {
            t.add("Terraform", "infrastructure", None, rel);
        }
        if rel.starts_with(".github/workflows/") {
            t.add("GitHub Actions", "ci", None, rel);
        }
        if rel == ".gitlab-ci.yml" {
            t.add("GitLab CI", "ci", None, rel);
        }
        if rel.starts_with("k8s/") || rel.starts_with("kubernetes/") || rel.starts_with("helm/") {
            t.add("Kubernetes", "infrastructure", None, rel);
        }
    }
    found
}

// ---- architecture -----------------------------------------------------------------------------

fn architecture(files: &[(String, u64)], techs: &[Technology]) -> Vec<Pattern> {
    // Every folder name, lowercased, with one example path where it occurs.
    let mut dirs: BTreeMap<String, String> = BTreeMap::new();
    for (rel, _) in files {
        let parts: Vec<&str> = rel.split('/').collect();
        for i in 0..parts.len().saturating_sub(1) {
            dirs.entry(parts[i].to_ascii_lowercase())
                .or_insert_with(|| parts[..=i].join("/"));
        }
    }
    let has = |names: &[&str]| -> Vec<String> {
        names
            .iter()
            .filter_map(|n| dirs.get(*n).map(|p| format!("{p}/")))
            .collect()
    };
    let tech = |name: &str| techs.iter().any(|t| t.name == name);
    let mut out = Vec::new();
    let mut push = |name, description, needed: usize, evidence: Vec<String>| {
        if evidence.len() >= needed {
            let confidence = (40 + evidence.len() * 15).min(95) as u8;
            out.push(Pattern {
                name,
                description,
                confidence,
                evidence,
            });
        }
    };

    push(
        "Clean Architecture / Hexagonal",
        "Domain at the center, with application, infrastructure and adapters (ports & adapters) around it.",
        2,
        has(&[
            "domain",
            "application",
            "infrastructure",
            "adapters",
            "ports",
            "usecases",
            "use_cases",
            "entities",
        ]),
    );
    push(
        "Layered (N-tier)",
        "Controllers or handlers call services, which call repositories or data access.",
        2,
        has(&[
            "controllers",
            "services",
            "repositories",
            "repository",
            "dao",
            "handlers",
        ]),
    );
    push(
        "MVC",
        "Models, views and controllers separated.",
        3,
        has(&["models", "views", "controllers"]),
    );
    push(
        "Feature-based (vertical slices)",
        "Code grouped by feature or module rather than by technical layer.",
        1,
        has(&["features", "modules", "domains", "slices"]),
    );
    push(
        "Monorepo",
        "Several packages or applications in one repository.",
        1,
        has(&["packages", "apps", "libs", "crates", "services"]),
    );
    push(
        "Component-based UI",
        "A user interface built from reusable components.",
        1,
        has(&["components", "widgets", "ui"]),
    );
    push(
        "API / Routes",
        "Exposes endpoints organized by routes.",
        1,
        has(&["routes", "api", "endpoints", "router"]),
    );
    push(
        "Event-driven",
        "Communicates through events, queues or message handlers.",
        1,
        has(&[
            "events",
            "listeners",
            "subscribers",
            "consumers",
            "producers",
            "queues",
        ]),
    );
    push(
        "CQRS",
        "Separates commands (writes) from queries (reads).",
        2,
        has(&["commands", "queries"]),
    );

    let dockerfiles = files
        .iter()
        .filter(|(f, _)| f.to_ascii_lowercase().ends_with("dockerfile"))
        .count();
    if dockerfiles >= 2 {
        out.push(Pattern {
            name: "Microservices",
            description: "Several independently built and deployed services.",
            confidence: (40 + dockerfiles * 10).min(90) as u8,
            evidence: vec![format!("{dockerfiles} Dockerfiles")],
        });
    }
    if tech("Tauri") || tech("Electron") {
        out.push(Pattern {
            name: "Desktop app (web frontend + native backend)",
            description: "A web UI running in a native shell that calls a native backend through commands/IPC.",
            confidence: 90,
            evidence: has(&["src-tauri", "electron"])
                .into_iter()
                .chain(std::iter::once("Tauri/Electron dependency".to_owned()))
                .collect(),
        });
    }
    let frontend = ["React", "Vue", "Svelte", "Angular", "SolidJS"]
        .iter()
        .any(|n| tech(n));
    let backend = [
        "Express",
        "Fastify",
        "NestJS",
        "Django",
        "Flask",
        "FastAPI",
        "Axum",
        "Actix Web",
        "Spring Boot",
        "Laravel",
        "Gin",
        "Ruby on Rails",
    ]
    .iter()
    .any(|n| tech(n));
    if frontend && backend {
        out.push(Pattern {
            name: "Full-stack (client / server)",
            description: "A frontend framework and a backend framework in the same project.",
            confidence: 80,
            evidence: vec!["frontend and backend frameworks".to_owned()],
        });
    }
    out.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    out
}

// ---- types for UML ----------------------------------------------------------------------------

fn ident(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect()
}

fn names(list: &str) -> Vec<String> {
    // Generic arguments are not other names: drop everything between angle brackets.
    let mut depth = 0;
    let mut flat = String::new();
    for c in list.chars() {
        match c {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth -= 1,
            _ if depth == 0 => flat.push(c),
            _ => {}
        }
    }
    flat.split(',')
        .map(|s| ident(s.trim().rsplit('.').next().unwrap_or("").trim()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// The words between `key` and the next of `stops`.
fn clause(line: &str, key: &str, stops: &[&str]) -> Option<String> {
    let at = line.find(key)? + key.len();
    let rest = &line[at..];
    let end = stops
        .iter()
        .filter_map(|s| rest.find(s))
        .min()
        .unwrap_or(rest.len());
    Some(rest[..end].to_owned())
}

fn strip_modifiers(line: &str) -> &str {
    let mut l = line.trim_start();
    loop {
        let before = l;
        for m in [
            "export ",
            "default ",
            "public ",
            "private ",
            "protected ",
            "internal ",
            "abstract ",
            "sealed ",
            "final ",
            "static ",
            "open ",
            "data ",
            "pub(crate) ",
            "pub(super) ",
            "pub ",
            "declare ",
            "partial ",
            "async ",
        ] {
            if let Some(r) = l.strip_prefix(m) {
                l = r.trim_start();
            }
        }
        if l == before {
            return l;
        }
    }
}

fn head(line: &str) -> Option<(&'static str, String, &str)> {
    let l = strip_modifiers(line);
    for (kw, kind) in [
        ("class ", "class"),
        ("interface ", "interface"),
        ("struct ", "struct"),
        ("enum ", "enum"),
        ("trait ", "trait"),
        ("record ", "class"),
        ("object ", "class"),
    ] {
        if let Some(rest) = l.strip_prefix(kw) {
            let name = ident(rest.trim_start());
            if !name.is_empty() && name.chars().next().is_some_and(|c| c.is_alphabetic()) {
                return Some((kind, name, rest));
            }
        }
    }
    None
}

fn member(line: &str, lang: &'static str) -> Option<(bool, String)> {
    let t = line.trim();
    if t.is_empty()
        || t.starts_with("//")
        || t.starts_with('#')
        || t.starts_with('*')
        || t.starts_with("/*")
        || t.starts_with('@')
        || t.starts_with("return")
        || t.starts_with('}')
    {
        return None;
    }
    let l = strip_modifiers(t);
    let l = l
        .strip_prefix("readonly ")
        .or_else(|| l.strip_prefix("override "))
        .unwrap_or(l);
    if let Some(rest) = l
        .strip_prefix("fn ")
        .or_else(|| l.strip_prefix("def "))
        .or_else(|| l.strip_prefix("func "))
        .or_else(|| l.strip_prefix("fun "))
        .or_else(|| l.strip_prefix("function "))
    {
        let n = ident(rest);
        return (!n.is_empty()).then(|| (true, format!("{n}()")));
    }
    let n = ident(l);
    if n.is_empty()
        || [
            "if", "for", "while", "switch", "else", "const", "let", "var", "self", "this", "super",
            "impl", "use", "match", "try", "catch", "new", "throw", "await", "case",
        ]
        .contains(&n.as_str())
    {
        return None;
    }
    let after = l[n.len()..].trim_start();
    if after.starts_with('(') || after.starts_with("<") && after.contains('(') {
        return Some((true, format!("{n}()")));
    }
    if lang == "Rust" || lang == "TypeScript" || lang == "Kotlin" || lang == "Swift" {
        if after.starts_with(':') || after.starts_with("?:") || after.starts_with('=') {
            let ty: String = after
                .trim_start_matches(['?', ':', ' '])
                .chars()
                .take_while(|c| *c != ',' && *c != ';' && *c != '=' && *c != '{')
                .collect();
            let ty = ty.trim();
            return Some((
                false,
                if ty.is_empty() || after.starts_with('=') {
                    n
                } else {
                    format!("{n}: {ty}")
                },
            ));
        }
        if after.is_empty() || after.starts_with(',') {
            return Some((false, n));
        }
        return None;
    }
    // Java, C#, PHP: `Type name;` or `Type name = ...;`
    let mut words = l.split_whitespace();
    if let (Some(ty), Some(name)) = (words.next(), words.next()) {
        let name = ident(name.trim_start_matches('$'));
        if !name.is_empty() && (l.ends_with(';') || l.contains(" = ")) {
            return Some((false, format!("{name}: {}", ty)));
        }
    }
    if lang == "Python" {
        if let Some(rest) = t.strip_prefix("self.") {
            let n = ident(rest);
            if !n.is_empty() && rest[n.len()..].trim_start().starts_with('=') {
                return Some((false, n));
            }
        }
    }
    None
}

fn source_lang(file: &str) -> Option<&'static str> {
    let ext = file.rsplit_once('.')?.1;
    Some(match ext {
        "ts" | "tsx" | "mts" => "TypeScript",
        "js" | "jsx" | "mjs" => "JavaScript",
        "rs" => "Rust",
        "py" => "Python",
        "java" => "Java",
        "kt" => "Kotlin",
        "cs" => "C#",
        "go" => "Go",
        "php" => "PHP",
        "swift" => "Swift",
        "dart" => "Dart",
        _ => return None,
    })
}

fn types_in(text: &str, file: &str, lang: &'static str, out: &mut Vec<TypeDecl>) {
    let lines: Vec<&str> = text.lines().collect();
    let mut rust_impls: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < lines.len() && out.len() < MAX_TYPES {
        let line = lines[i];
        // Go: `type Name struct {` / `type Name interface {`
        if lang == "Go" {
            if let Some(rest) = line.trim_start().strip_prefix("type ") {
                let name = ident(rest);
                let after = rest[name.len()..].trim_start();
                let kind = if after.starts_with("struct") {
                    Some("struct")
                } else if after.starts_with("interface") {
                    Some("interface")
                } else {
                    None
                };
                if let (Some(kind), false) = (kind, name.is_empty()) {
                    let mut decl = decl(name, kind, file, lang);
                    let mut j = i + 1;
                    while j < lines.len() && !lines[j].trim_start().starts_with('}') {
                        let t = lines[j].trim();
                        let mut w = t.split_whitespace();
                        if let Some(n) = w.next() {
                            let n = ident(n);
                            if !n.is_empty() && decl.fields.len() + decl.methods.len() < MAX_MEMBERS
                            {
                                match (kind, w.next()) {
                                    ("interface", _) => decl.methods.push(format!("{n}()")),
                                    (_, Some(ty)) => decl.fields.push(format!("{n}: {ty}")),
                                    (_, None) => decl.extends.push(n),
                                }
                            }
                        }
                        j += 1;
                    }
                    out.push(decl);
                    i = j;
                    continue;
                }
            }
            if let Some(rest) = line.strip_prefix("func (") {
                // A method: attach it to its receiver later.
                if let Some(close) = rest.find(')') {
                    let recv = rest[..close].split_whitespace().last().unwrap_or("");
                    let recv = ident(recv.trim_start_matches('*'));
                    let m = ident(rest[close + 1..].trim_start());
                    if !recv.is_empty() && !m.is_empty() {
                        rust_impls.push((recv, format!("@{m}()")));
                    }
                }
            }
            i += 1;
            continue;
        }
        // Rust: `impl Trait for Type` / `impl Type` and their `fn`s.
        if lang == "Rust" {
            let t = line.trim_start();
            if t.starts_with("impl") && (t.starts_with("impl ") || t.starts_with("impl<")) {
                let body = if t.starts_with("impl<") {
                    let mut depth = 0;
                    let mut end = 4;
                    for (k, c) in t.char_indices().skip(4) {
                        match c {
                            '<' => depth += 1,
                            '>' => {
                                depth -= 1;
                                if depth == 0 {
                                    end = k + 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    &t[end..]
                } else {
                    &t[5..]
                };
                let body = body
                    .split('{')
                    .next()
                    .unwrap_or("")
                    .split(" where")
                    .next()
                    .unwrap_or("");
                let (tr, ty) = match body.split_once(" for ") {
                    Some((tr, ty)) => (Some(tr.trim()), ty.trim()),
                    None => (None, body.trim()),
                };
                let ty = ident(ty.trim_start_matches('&').rsplit("::").next().unwrap_or(""));
                if !ty.is_empty() {
                    if let Some(tr) = tr {
                        let tr = ident(tr.rsplit("::").next().unwrap_or(""));
                        if !tr.is_empty() {
                            rust_impls.push((ty.clone(), format!(":{tr}")));
                        }
                    }
                    let indent = line.len() - t.len();
                    let mut j = i + 1;
                    while j < lines.len() {
                        let l = lines[j];
                        let lt = l.trim_start();
                        if lt.starts_with('}') && l.len() - lt.len() == indent {
                            break;
                        }
                        if l.len() - lt.len() == indent + 4 {
                            let s = strip_modifiers(lt);
                            if let Some(r) = s.strip_prefix("fn ") {
                                let n = ident(r);
                                if !n.is_empty() {
                                    rust_impls.push((ty.clone(), format!("@{n}()")));
                                }
                            }
                        }
                        j += 1;
                    }
                    i = j + 1;
                    continue;
                }
            }
        }
        let Some((kind, name, rest)) = head(line) else {
            i += 1;
            continue;
        };
        let header: String = {
            // A header can wrap until its opening brace or colon.
            let mut h = rest.to_owned();
            let mut j = i;
            while !h.contains('{')
                && !h.trim_end().ends_with(':')
                && !h.contains(';')
                && j + 1 < lines.len()
                && j < i + 4
            {
                j += 1;
                h.push(' ');
                h.push_str(lines[j].trim());
            }
            h
        };
        let mut decl = decl(name, kind, file, lang);
        let stops = ["{", " implements ", " where ", ":", "=>", ";"];
        match lang {
            "Python" => {
                if let Some(base) = clause(&header, "(", &[")"]) {
                    decl.extends = names(&base)
                        .into_iter()
                        .filter(|n| {
                            n != "object" && n != "ABC" && n != "Protocol" && n != "metaclass"
                        })
                        .collect();
                }
            }
            "Kotlin" | "Swift" | "Dart" if header.contains(':') && lang != "Dart" => {
                decl.extends = names(&clause(&header, ":", &["{"]).unwrap_or_default());
            }
            "Rust" => {
                if kind == "trait" {
                    if let Some(c) = clause(&header, ":", &["{", " where "]) {
                        decl.extends = c
                            .split('+')
                            .map(|s| ident(s.trim()))
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
            }
            "C#" => {
                if let Some(c) = clause(&header, ":", &["{", " where "]) {
                    let list = names(&c);
                    for n in list {
                        if n.len() > 1
                            && n.starts_with('I')
                            && n.chars().nth(1).is_some_and(|c| c.is_uppercase())
                        {
                            decl.implements.push(n);
                        } else {
                            decl.extends.push(n);
                        }
                    }
                }
            }
            _ => {
                if let Some(c) = clause(&header, " extends ", &stops) {
                    decl.extends = names(&c);
                }
                if let Some(c) = clause(&header, " implements ", &["{"]) {
                    decl.implements = names(&c);
                }
                if lang == "Dart" {
                    if let Some(c) = clause(&header, " with ", &["{", " implements "]) {
                        decl.implements.extend(names(&c));
                    }
                }
            }
        }
        // The body: members at the first indentation inside it.
        let indent = line.len() - line.trim_start().len();
        let single_line = header.contains('{') && header.trim_end().ends_with('}');
        let mut j = i + 1;
        let mut member_indent: Option<usize> = None;
        if !single_line && !header.trim_end().ends_with(';') {
            while j < lines.len() {
                let l = lines[j];
                let lt = l.trim_start();
                let ind = l.len() - lt.len();
                if !lt.is_empty() && ind <= indent && !(lt.starts_with('{') && ind == indent) {
                    if lt.starts_with('}') || lt.starts_with(')') {
                        j += 1;
                    }
                    break;
                }
                if !lt.is_empty() {
                    let mi = *member_indent.get_or_insert(ind);
                    if ind == mi && decl.fields.len() + decl.methods.len() < MAX_MEMBERS {
                        if kind == "enum" {
                            let n = ident(lt);
                            if !n.is_empty() && n.chars().next().is_some_and(|c| c.is_uppercase()) {
                                decl.fields.push(n);
                            }
                        } else if let Some((is_method, m)) = member(lt, lang) {
                            if is_method {
                                if !decl.methods.contains(&m) {
                                    decl.methods.push(m);
                                }
                            } else {
                                decl.fields.push(m);
                            }
                        }
                    }
                }
                j += 1;
            }
        }
        out.push(decl);
        i = j.max(i + 1);
    }
    for (ty, what) in rust_impls {
        if let Some(d) = out
            .iter_mut()
            .rev()
            .find(|d| d.name == ty && d.file == file)
        {
            if let Some(tr) = what.strip_prefix(':') {
                if !d.implements.iter().any(|x| x == tr) {
                    d.implements.push(tr.to_owned());
                }
            } else if let Some(m) = what.strip_prefix('@') {
                if d.methods.len() + d.fields.len() < MAX_MEMBERS
                    && !d.methods.iter().any(|x| x == m)
                {
                    d.methods.push(m.to_owned());
                }
            }
        }
    }
}

fn decl(name: String, kind: &'static str, file: &str, lang: &'static str) -> TypeDecl {
    TypeDecl {
        name,
        kind,
        file: file.to_owned(),
        language: lang,
        extends: Vec::new(),
        implements: Vec::new(),
        fields: Vec::new(),
        methods: Vec::new(),
    }
}

fn types(root: &Path, files: &[(String, u64)]) -> Vec<TypeDecl> {
    let mut out = Vec::new();
    let mut read = 0;
    for (rel, size) in files {
        if out.len() >= MAX_TYPES || read >= MAX_SOURCE_FILES {
            break;
        }
        let Some(lang) = source_lang(rel) else {
            continue;
        };
        if *size > MAX_SOURCE_BYTES || rel.ends_with(".d.ts") || rel.contains(".min.") {
            continue;
        }
        let Some(text) = read_small(root, rel) else {
            continue;
        };
        read += 1;
        types_in(&text, rel, lang, &mut out);
    }
    out
}

// ---- README -----------------------------------------------------------------------------------

fn readme_name(root: &Path) -> Option<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_ascii_lowercase().starts_with("readme"))
        .collect();
    names.sort_by_key(|n| (!n.to_ascii_lowercase().ends_with(".md"), n.len()));
    names.into_iter().next()
}

fn readme(root: &Path) -> Option<Readme> {
    let name = readme_name(root)?;
    let bytes = fs::read(root.join(&name)).ok()?;
    let truncated = bytes.len() > MAX_README_BYTES;
    let slice = &bytes[..bytes.len().min(MAX_README_BYTES)];
    Some(Readme {
        name,
        content: String::from_utf8_lossy(slice).into_owned(),
        truncated,
    })
}

/// Writes the project's README (the one found, or `README.md`). Nothing else can be written.
pub fn save_readme(root: &str, content: &str) -> Result<String, Failure> {
    if content.len() > MAX_README_BYTES {
        return Err(Failure::new(ErrorCode::CapacityExceeded));
    }
    let root = Path::new(root);
    let name = readme_name(root).unwrap_or_else(|| "README.md".to_owned());
    let path = root.join(&name);
    // A README that is a link would write outside the folder.
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
        return Err(Failure::new(ErrorCode::RootIneligible));
    }
    let tmp = root.join(format!(".{name}.wyss-tmp"));
    fs::write(&tmp, content.as_bytes()).map_err(|_| io())?;
    fs::rename(&tmp, &path).map_err(|_| {
        let _ = fs::remove_file(&tmp);
        io()
    })?;
    Ok(name)
}

// ---- the whole answer -------------------------------------------------------------------------

pub fn inspect(root: &str) -> Result<Insight, Failure> {
    let root = Path::new(root);
    if !root.is_dir() {
        return Err(Failure::new(ErrorCode::RootMissing));
    }
    let w = walk(root);
    let mut langs: BTreeMap<&'static str, (usize, u64)> = BTreeMap::new();
    let mut structure: BTreeMap<String, (bool, usize)> = BTreeMap::new();
    let mut bytes = 0;
    for (rel, size) in &w.files {
        bytes += size;
        if let Some(l) = language_of(rel) {
            let e = langs.entry(l).or_default();
            e.0 += 1;
            e.1 += size;
        }
        match rel.split_once('/') {
            Some((top, _)) => structure.entry(top.to_owned()).or_insert((true, 0)).1 += 1,
            None => {
                structure.insert(rel.clone(), (false, 1));
            }
        }
    }
    let mut languages: Vec<Language> = langs
        .into_iter()
        .map(|(name, (files, bytes))| Language {
            name: name.to_owned(),
            files,
            bytes,
        })
        .collect();
    languages.sort_by(|a, b| b.bytes.cmp(&a.bytes));

    let mut t = Techs {
        list: Vec::new(),
        seen: BTreeSet::new(),
        scripts: Vec::new(),
    };
    let manifests = manifests(root, &w.files, &mut t);
    let architecture = architecture(&w.files, &t.list);
    let license = w
        .files
        .iter()
        .find(|(f, _)| {
            let l = f.to_ascii_lowercase();
            !f.contains('/') && (l.starts_with("license") || l.starts_with("licence"))
        })
        .and_then(|(f, _)| read_small(root, f))
        .and_then(|text| {
            let first = text
                .lines()
                .find(|l| !l.trim().is_empty())?
                .trim()
                .to_owned();
            Some(first.chars().take(120).collect())
        });
    let mut structure: Vec<Entry> = structure
        .into_iter()
        .map(|(name, (dir, files))| Entry { name, dir, files })
        .collect();
    structure.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.name.cmp(&b.name)));

    Ok(Insight {
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        files: w.files.len(),
        folders: w.folders,
        bytes,
        truncated: w.truncated,
        languages,
        technologies: t.list,
        architecture,
        structure,
        manifests,
        scripts: t.scripts,
        git: root.join(".git").exists(),
        license,
        readme: readme(root),
        types: types(root, &w.files),
    })
}

#[cfg(test)]
mod insight_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wyss-insight-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_technologies_architecture_and_types() {
        let dir = scratch("all");
        fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"react":"18.3.1","express":"4"},"devDependencies":{"vitest":"1"},"scripts":{"dev":"vite"}}"#,
        )
        .unwrap();
        fs::create_dir_all(dir.join("src/domain")).unwrap();
        fs::create_dir_all(dir.join("src/infrastructure")).unwrap();
        fs::write(
            dir.join("src/domain/user.ts"),
            "export interface Named {\n  name: string;\n}\n\nexport class User extends Base implements Named {\n  name: string;\n  greet(): string {\n    return this.name;\n  }\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/infrastructure/repo.rs"),
            "pub struct Repo {\n    pub items: Vec<u8>,\n}\n\nimpl Store for Repo {\n    fn load(&self) {}\n}\n",
        )
        .unwrap();
        fs::write(dir.join("README.md"), "# Hello\n").unwrap();

        let i = inspect(dir.to_str().unwrap()).unwrap();
        let names: Vec<_> = i.technologies.iter().map(|t| t.name.as_str()).collect();
        assert!(
            names.contains(&"React") && names.contains(&"Express") && names.contains(&"Vitest")
        );
        assert!(i.architecture.iter().any(|p| p.name.starts_with("Clean")));
        assert!(
            i.architecture
                .iter()
                .any(|p| p.name.starts_with("Full-stack"))
        );
        assert_eq!(i.readme.as_ref().unwrap().content, "# Hello\n");
        let user = i.types.iter().find(|t| t.name == "User").unwrap();
        assert_eq!(user.extends, vec!["Base"]);
        assert_eq!(user.implements, vec!["Named"]);
        assert!(user.methods.contains(&"greet()".to_owned()));
        let repo = i.types.iter().find(|t| t.name == "Repo").unwrap();
        assert_eq!(repo.implements, vec!["Store"]);
        assert!(repo.methods.contains(&"load()".to_owned()));
        assert!(repo.fields.iter().any(|f| f.starts_with("items")));

        assert_eq!(
            save_readme(dir.to_str().unwrap(), "# Changed\n").unwrap(),
            "README.md"
        );
        assert_eq!(
            fs::read_to_string(dir.join("README.md")).unwrap(),
            "# Changed\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
