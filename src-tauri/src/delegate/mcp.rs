//! `wyss-console --wyss-delegate-mcp`: the delegation tool as an MCP server over stdio, for an
//! orchestrator session Wyss started (Claude Code through `--mcp-config`, Codex through `-c
//! mcp_servers…`). It holds no state and no right of its own: every call goes to the running
//! terminal host with the session's token from the environment, and the host decides. It never
//! starts a host and never falls back to running one in-process.
use crate::host::protocol::{Answer, Request as HostRequest, read_frame, write_frame};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const CALL_TIMEOUT: Duration = Duration::from_secs(20);
/// Longest one wait call blocks; the orchestrator can call it again.
const MAX_WAIT: u64 = 600;

fn host(op: &str, params: Value) -> Result<Value, String> {
    let path = crate::host::server::socket_path().ok_or("no runtime directory")?;
    let mut stream =
        UnixStream::connect(path).map_err(|_| "the Wyss terminal host is not running")?;
    let _ = stream.set_read_timeout(Some(CALL_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CALL_TIMEOUT));
    write_frame(
        &mut stream,
        &HostRequest {
            op: op.to_owned(),
            params,
        },
    )
    .map_err(|_| "the Wyss terminal host did not answer")?;
    match read_frame::<Answer>(&mut stream).map_err(|_| "the Wyss terminal host did not answer")? {
        Answer::Ok(value) => Ok(value),
        Answer::Err(error) => Err(if error.reasons.is_empty() {
            error.code
        } else {
            format!("{}: {}", error.code, error.reasons.join("; "))
        }),
    }
}

fn token() -> Option<String> {
    std::env::var(super::TOKEN_VAR)
        .ok()
        .filter(|t| !t.is_empty())
}

fn tools() -> Value {
    let provider = json!({"type": "string", "enum": ["codex", "claude_code"]});
    json!([
        {
            "name": "wyss_delegate_options",
            "description": "Models, reasoning efforts and limits a delegated agent of this provider can use here, as the installed provider and the owner's policy allow.",
            "inputSchema": {"type": "object", "properties": {"provider": provider}, "required": ["provider"]}
        },
        {
            "name": "wyss_arsenal_capabilities",
            "description": "Arsenal skills a delegated agent of this provider can be given: capability id, name, current revision, compatibility.",
            "inputSchema": {"type": "object", "properties": {"provider": provider, "query": {"type": "string"}}, "required": ["provider"]}
        },
        {
            "name": "wyss_delegate",
            "description": "Start a task with a separate Codex or Claude Code agent run by Wyss Console in its own workspace. Choose its model and reasoning effort (see wyss_delegate_options) and its skills: Arsenal capability ids (optionally pinned revisions), an Arsenal preset or selection. The agent gets only those skills, writes only in its workspace, has no network for its tools and never asks for approval. Returns the delegation id at once; then call wyss_delegation_wait.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "provider": provider,
                    "model": {"type": "string"},
                    "effort": {"type": "string"},
                    "task": {"type": "string"},
                    "title": {"type": "string"},
                    "skills": {"type": "array", "items": {"type": "object", "properties": {"capability": {"type": "string"}, "revision": {"type": "integer"}}, "required": ["capability"]}},
                    "preset": {"type": "string"},
                    "selection": {"type": "string"},
                    "timeout_minutes": {"type": "integer"}
                },
                "required": ["provider", "model", "effort", "task"]
            }
        },
        {
            "name": "wyss_delegation_wait",
            "description": "Wait for a delegation to finish (up to wait_seconds, at most 600) and return its status, result, artifacts, applied configuration and its verification.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "wait_seconds": {"type": "integer"}}, "required": ["id"]}
        },
        {
            "name": "wyss_delegation_status",
            "description": "The current status of one delegation, without waiting.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}
        },
        {
            "name": "wyss_delegation_cancel",
            "description": "Stop a running delegation and its processes.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}
        },
        {
            "name": "wyss_delegation_list",
            "description": "Delegations started by this session, newest first.",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}

/// What one tool call answers: the text the model reads, and whether it is an error.
fn call(name: &str, args: &Value) -> (String, bool) {
    let Some(token) = token() else {
        return (
            "This session was not started from Wyss Console with delegation on, so it cannot delegate.".into(),
            true,
        );
    };
    let id = || args["id"].as_str().unwrap_or_default().to_owned();
    let outcome = match name {
        "wyss_delegate_options" => host("delegate_options", json!({"provider": args["provider"]})),
        "wyss_arsenal_capabilities" => host(
            "delegate_capabilities",
            json!({"provider": args["provider"], "query": args["query"]}),
        ),
        "wyss_delegate" => {
            let mut request = args.clone();
            if let Some(map) = request.as_object_mut() {
                map.insert("token".into(), Value::String(token));
            }
            host("delegate_start", request).map(|record| brief(&record))
        }
        "wyss_delegation_status" => {
            host("delegate_get", json!({"id": id(), "token": token})).map(|r| brief(&r))
        }
        "wyss_delegation_cancel" => {
            host("delegate_cancel", json!({"id": id(), "token": token})).map(|r| brief(&r))
        }
        "wyss_delegation_list" => host("delegate_list", json!({"token": token}))
            .map(|list| Value::Array(list.as_array().into_iter().flatten().map(brief).collect())),
        "wyss_delegation_wait" => {
            let wait = args["wait_seconds"].as_u64().unwrap_or(120).min(MAX_WAIT);
            let deadline = Instant::now() + Duration::from_secs(wait);
            loop {
                match host("delegate_get", json!({"id": id(), "token": token})) {
                    Ok(record) if record["status"] != "running" || Instant::now() >= deadline => {
                        break Ok(record);
                    }
                    Ok(_) => std::thread::sleep(Duration::from_secs(2)),
                    Err(error) => break Err(error),
                }
            }
        }
        other => Err(format!("unknown tool {other}")),
    };
    match outcome {
        Ok(value) => (
            serde_json::to_string_pretty(&value).unwrap_or_default(),
            false,
        ),
        Err(error) => (error, true),
    }
}

/// A record as the orchestrator needs it: everything but the task text it sent itself.
fn brief(record: &Value) -> Value {
    let mut record = record.clone();
    if let Some(map) = record.as_object_mut() {
        map.remove("task");
    }
    record
}

/// Serves MCP over stdin/stdout until stdin closes. Returns the process exit code.
pub fn run() -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = message.get("id").cloned() else {
            continue; // a notification
        };
        let method = message["method"].as_str().unwrap_or_default();
        let reply = match method {
            "initialize" => json!({"jsonrpc": "2.0", "id": id, "result": {
                "protocolVersion": message["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "wyss-delegate", "version": env!("CARGO_PKG_VERSION")}
            }}),
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            "tools/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"tools": tools()}}),
            "tools/call" => {
                let name = message["params"]["name"].as_str().unwrap_or_default();
                let (text, error) = call(name, &message["params"]["arguments"]);
                json!({"jsonrpc": "2.0", "id": id, "result": {
                    "content": [{"type": "text", "text": text}],
                    "isError": error
                }})
            }
            _ => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}})
            }
        };
        if writeln!(stdout, "{reply}")
            .and_then(|()| stdout.flush())
            .is_err()
        {
            break;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_declares_an_object_schema_and_the_start_needs_the_four_essentials() {
        let tools = tools();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 7);
        for tool in tools.as_array().unwrap() {
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
        let start = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "wyss_delegate")
            .unwrap();
        assert_eq!(
            start["inputSchema"]["required"],
            json!(["provider", "model", "effort", "task"])
        );
    }

    #[test]
    fn without_the_token_nothing_is_asked_of_the_host() {
        // The test process has no delegation token.
        unsafe { std::env::remove_var(super::super::TOKEN_VAR) };
        let (text, error) = call("wyss_delegate", &json!({"provider": "codex"}));
        assert!(error);
        assert!(text.contains("cannot delegate"));
        assert!(
            brief(&json!({"task": "secret", "id": "x"}))
                .get("task")
                .is_none()
        );
    }
}
