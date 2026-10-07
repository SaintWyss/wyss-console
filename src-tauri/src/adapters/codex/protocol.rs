use super::{CodecError, Result, fail, wire::*};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Stable,
    Experimental,
    Unknown,
    PrivateReasoning,
}
pub fn method_surface(method: &str) -> Surface {
    match method {
        "initialize"
        | "initialized"
        | "thread/start"
        | "thread/resume"
        | "thread/archive"
        | "thread/unarchive"
        | "turn/start"
        | "turn/interrupt"
        | "model/list"
        | "thread/started"
        | "thread/archived"
        | "thread/unarchived"
        | "turn/started"
        | "turn/completed"
        | "item/started"
        | "item/completed"
        | "item/agentMessage/delta"
        | "item/commandExecution/outputDelta"
        | "item/mcpToolCall/progress"
        | "item/reasoning/summaryTextDelta"
        | "turn/plan/updated"
        | "item/commandExecution/requestApproval"
        | "item/fileChange/requestApproval"
        | "item/permissions/requestApproval"
        | "serverRequest/resolved" => Surface::Stable,
        "item/plan/delta"
        | "process/outputDelta"
        | "item/tool/call"
        | "item/tool/requestUserInput" => Surface::Experimental,
        "item/reasoning/textDelta" => Surface::PrivateReasoning,
        _ => Surface::Unknown,
    }
}
/// Conservative feature classification for the implemented method subset.
/// Unknown fields are retained by the wire envelope, never advertised as coverage.
pub fn message_surface(message: &WireMessage) -> Surface {
    let Some(method) = message.method() else {
        return Surface::Unknown;
    };
    let surface = method_surface(method);
    if surface != Surface::Stable {
        return surface;
    }
    let Some(p) = message.params().and_then(Value::as_object) else {
        return surface;
    };
    let experimental_fields: &[&str] = match method {
        "thread/start" => &[
            "allowProviderModelFallback",
            "runtimeWorkspaceRoots",
            "permissions",
            "multiAgentMode",
            "historyMode",
            "projectId",
            "daybreakEnabled",
            "environments",
            "dynamicTools",
            "selectedCapabilityRoots",
            "mockExperimentalField",
            "experimentalRawEvents",
        ],
        "thread/resume" => &[
            "history",
            "path",
            "runtimeWorkspaceRoots",
            "permissions",
            "initialTurnsPage",
        ],
        "turn/start" => &[
            "responsesapiClientMetadata",
            "additionalContext",
            "environments",
            "runtimeWorkspaceRoots",
            "permissions",
            "collaborationMode",
            "multiAgentMode",
            "cyberAccessProgram",
        ],
        "item/commandExecution/requestApproval" => &["additionalPermissions"],
        "item/fileChange/requestApproval" => &["grantRoot"],
        _ => &[],
    };
    if experimental_fields
        .iter()
        .any(|k| p.get(*k).is_some_and(|v| !v.is_null()))
        || p.get("item").is_some_and(|i| i["type"] == "plan")
    {
        return Surface::Experimental;
    }
    surface
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Initialize,
    ThreadStart,
    ThreadResume,
    ThreadArchive,
    ThreadUnarchive,
    TurnStart,
    TurnInterrupt,
    ModelList,
    CommandApproval,
    FileApproval,
    PermissionApproval,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub title: Option<String>,
    pub version: String,
}
#[derive(Debug, Clone)]
pub enum ClientOperation {
    Initialize {
        client_info: ClientInfo,
        experimental_api: bool,
    },
    ThreadStart {
        cwd: Option<String>,
        model: Option<String>,
        model_provider: Option<String>,
    },
    ThreadResume {
        thread_id: CodexThreadId,
    },
    TurnStart {
        thread_id: CodexThreadId,
        text: String,
    },
    TurnInterrupt {
        thread_id: CodexThreadId,
        turn_id: CodexTurnId,
    },
    ModelList {
        limit: Option<u32>,
        cursor: Option<String>,
        include_hidden: bool,
    },
}
impl ClientOperation {
    pub fn operation(&self) -> Operation {
        match self {
            Self::Initialize { .. } => Operation::Initialize,
            Self::ThreadStart { .. } => Operation::ThreadStart,
            Self::ThreadResume { .. } => Operation::ThreadResume,
            Self::TurnStart { .. } => Operation::TurnStart,
            Self::TurnInterrupt { .. } => Operation::TurnInterrupt,
            Self::ModelList { .. } => Operation::ModelList,
        }
    }
    pub fn encode(&self, id: CodexRequestId) -> Result<WireMessage> {
        let (method, params) = match self {
            Self::Initialize {
                client_info,
                experimental_api,
            } => (
                "initialize",
                json!({"clientInfo":client_info,"capabilities":{"experimentalApi":experimental_api}}),
            ),
            Self::ThreadStart {
                cwd,
                model,
                model_provider,
            } => (
                "thread/start",
                json!({"cwd":cwd,"model":model,"modelProvider":model_provider}),
            ),
            Self::ThreadResume { thread_id } => ("thread/resume", json!({"threadId":thread_id})),
            Self::TurnStart { thread_id, text } => (
                "turn/start",
                json!({"threadId":thread_id,"input":[{"type":"text","text":text,"text_elements":[]}]}),
            ),
            Self::TurnInterrupt { thread_id, turn_id } => (
                "turn/interrupt",
                json!({"threadId":thread_id,"turnId":turn_id}),
            ),
            Self::ModelList {
                limit,
                cursor,
                include_hidden,
            } => (
                "model/list",
                json!({"cursor":cursor,"includeHidden":include_hidden,"limit":limit}),
            ),
        };
        WireMessage::request(id, method, params)
    }
}
pub fn initialized_notification() -> Result<WireMessage> {
    WireMessage::from_value(json!({"method":"initialized"}), Direction::ClientToServer)
}

pub(crate) fn params(m: &WireMessage) -> Result<&Map<String, Value>> {
    object(m.params().ok_or(CodecError {
        code: "missing_params",
    })?)
}
pub(crate) fn field_text<'a>(p: &'a Map<String, Value>, name: &str, max: usize) -> Result<&'a str> {
    text(required(p, name)?, max)
}
pub(crate) fn ids(p: &Map<String, Value>, turn: bool, item: bool) -> Result<()> {
    CodexThreadId::parse(field_text(p, "threadId", 256)?)?;
    if turn {
        CodexTurnId::parse(field_text(p, "turnId", 256)?)?;
    }
    if item {
        CodexItemId::parse(field_text(p, "itemId", 256)?)?;
    }
    Ok(())
}
fn optional_text(p: &Map<String, Value>, name: &str, max: usize) -> Result<()> {
    if let Some(v) = p.get(name).filter(|v| !v.is_null()) {
        text(v, max)?;
    }
    Ok(())
}
fn boolean(v: &Value) -> Result<()> {
    if v.is_boolean() {
        Ok(())
    } else {
        fail("invalid_boolean")
    }
}
fn optional_bool(p: &Map<String, Value>, name: &str) -> Result<()> {
    if let Some(v) = p.get(name).filter(|v| !v.is_null()) {
        boolean(v)?;
    }
    Ok(())
}
fn client_params(method: &str, p: &Map<String, Value>) -> Result<()> {
    match method {
        "initialize" => {
            let c = object(required(p, "clientInfo")?)?;
            for key in ["name", "version"] {
                if field_text(c, key, 128)?.is_empty() {
                    return fail("invalid_client_info");
                }
            }
            optional_text(c, "title", 128)?;
            if let Some(v) = p.get("capabilities").filter(|v| !v.is_null()) {
                let c = object(v)?;
                if let Some(v) = c.get("experimentalApi") {
                    boolean(v)?;
                }
            }
        }
        "thread/start" => {
            for k in ["cwd", "model", "modelProvider"] {
                optional_text(p, k, 2048)?;
            }
        }
        "thread/resume" => ids(p, false, false)?,
        "thread/archive" | "thread/unarchive" => {
            only(p, &["threadId"])?;
            ids(p, false, false)?;
        }
        "turn/start" => {
            ids(p, false, false)?;
            for input in array(required(p, "input")?)? {
                let i = object(input)?;
                // The first outbound codec deliberately supports text only.
                if field_text(i, "type", 128)? != "text" {
                    return fail("unsupported_input");
                }
                field_text(i, "text", MAX_CONTENT_BYTES)?;
                if let Some(v) = i.get("text_elements") {
                    array(v)?;
                }
            }
        }
        "turn/interrupt" => ids(p, true, false)?,
        "model/list" => {
            optional_text(p, "cursor", 2048)?;
            optional_bool(p, "includeHidden")?;
            if let Some(v) = p.get("limit").filter(|v| !v.is_null())
                && !v.as_u64().is_some_and(|n| n <= u64::from(u32::MAX))
            {
                return fail("invalid_limit");
            }
        }
        _ => (),
    }
    Ok(())
}
pub(crate) fn validate_message(m: &WireMessage) -> Result<()> {
    let Some(method) = m.method() else {
        return Ok(());
    };
    let client = matches!(
        method,
        "initialize"
            | "thread/start"
            | "thread/resume"
            | "thread/archive"
            | "thread/unarchive"
            | "turn/start"
            | "turn/interrupt"
            | "model/list"
    );
    if client {
        if m.kind() != WireKind::ClientRequest {
            return fail("wrong_direction_or_kind");
        }
        return client_params(method, params(m)?);
    }
    if method == "initialized" {
        if m.kind() != WireKind::Notification
            || m.direction() != Direction::ClientToServer
            || m.params().is_some_and(|v| !v.is_null())
        {
            return fail("invalid_initialized");
        }
        return Ok(());
    }
    if let Some(approval) = approval_kind(method) {
        if m.kind() != WireKind::ServerRequest {
            return fail("wrong_direction_or_kind");
        }
        let p = params(m)?;
        ids(p, true, true)?;
        integer(required(p, "startedAtMs")?)?;
        for k in [
            "reason",
            "command",
            "cwd",
            "approvalId",
            "environmentId",
            "grantRoot",
        ] {
            optional_text(p, k, MAX_CONTENT_BYTES)?;
        }
        match approval {
            ApprovalKind::Command => {
                if let Some(v) = p.get("kind") {
                    one_of(v, &["command", "writeStdin"])?;
                }
                if let Some(v) = p.get("commandActions").filter(|v| !v.is_null()) {
                    array(v)?;
                }
            }
            ApprovalKind::FileChange => (),
            ApprovalKind::Permissions => {
                field_text(p, "cwd", 2048)?;
                permission_profile(required(p, "permissions")?)?;
            }
        }
        return Ok(());
    }
    if method_surface(method) != Surface::Stable {
        return Ok(());
    }
    if m.kind() != WireKind::Notification || m.direction() != Direction::ServerToClient {
        return fail("wrong_direction_or_kind");
    }
    let p = params(m)?;
    match method {
        "thread/started" => validate_thread(required(p, "thread")?)?,
        "thread/archived" | "thread/unarchived" => ids(p, false, false)?,
        "turn/started" | "turn/completed" => {
            ids(p, false, false)?;
            validate_turn(required(p, "turn")?)?;
            let status = &p["turn"]["status"];
            if method == "turn/completed" && status == "inProgress" {
                return fail("invalid_final_status");
            }
            if method == "turn/started" && status != "inProgress" {
                return fail("invalid_start_status");
            }
        }
        "item/started" | "item/completed" => {
            ids(p, true, false)?;
            validate_item(required(p, "item")?)?;
            integer(required(
                p,
                if method == "item/started" {
                    "startedAtMs"
                } else {
                    "completedAtMs"
                },
            )?)?;
        }
        "item/agentMessage/delta"
        | "item/commandExecution/outputDelta"
        | "item/reasoning/summaryTextDelta" => {
            ids(p, true, true)?;
            field_text(p, "delta", MAX_CONTENT_BYTES)?;
            if method == "item/reasoning/summaryTextDelta" {
                integer(required(p, "summaryIndex")?)?;
            }
        }
        "item/mcpToolCall/progress" => {
            ids(p, true, true)?;
            field_text(p, "message", MAX_CONTENT_BYTES)?;
        }
        "turn/plan/updated" => {
            ids(p, true, false)?;
            optional_text(p, "explanation", MAX_CONTENT_BYTES)?;
            for step in array(required(p, "plan")?)? {
                let s = object(step)?;
                field_text(s, "step", MAX_CONTENT_BYTES)?;
                one_of(
                    required(s, "status")?,
                    &["pending", "inProgress", "completed"],
                )?;
            }
        }
        "serverRequest/resolved" => {
            ids(p, false, false)?;
            CodexRequestId::from_value(required(p, "requestId")?)?;
        }
        _ => (),
    }
    Ok(())
}
pub(crate) fn validate_thread(v: &Value) -> Result<()> {
    let t = object(v)?;
    CodexThreadId::parse(field_text(t, "id", 256)?)?;
    for k in ["cliVersion", "cwd", "modelProvider", "preview", "sessionId"] {
        field_text(t, k, MAX_CONTENT_BYTES)?;
    }
    for k in ["createdAt", "updatedAt"] {
        integer(required(t, k)?)?;
    }
    boolean(required(t, "ephemeral")?)?;
    optional_text(t, "projectId", 256)?;
    required(t, "projectId")?;
    let source = required(t, "source")?;
    if !source.is_string() && !source.is_object() {
        return fail("invalid_thread_source");
    }
    let status = object(required(t, "status")?)?;
    field_text(status, "type", 128)?;
    if status["type"] == "active" {
        for f in array(required(status, "activeFlags")?)? {
            text(f, 128)?;
        }
    }
    for turn in array(required(t, "turns")?)? {
        validate_turn(turn)?;
    }
    optional_text(t, "model", 256)?;
    Ok(())
}
pub(crate) fn validate_turn(v: &Value) -> Result<()> {
    let t = object(v)?;
    CodexTurnId::parse(field_text(t, "id", 256)?)?;
    one_of(
        required(t, "status")?,
        &["inProgress", "completed", "failed", "interrupted"],
    )?;
    for i in array(required(t, "items")?)? {
        validate_item(i)?;
    }
    if let Some(v) = t.get("error").filter(|v| !v.is_null()) {
        let e = object(v)?;
        field_text(e, "message", 4096)?;
    }
    Ok(())
}
pub(crate) fn validate_item(v: &Value) -> Result<()> {
    let i = object(v)?;
    CodexItemId::parse(field_text(i, "id", 256)?)?;
    let kind = field_text(i, "type", 128)?;
    match kind {
        "agentMessage" | "plan" => {
            field_text(i, "text", MAX_CONTENT_BYTES)?;
            if kind == "agentMessage" {
                optional_text(i, "phase", 128)?;
            }
        }
        "userMessage" => {
            for content in array(required(i, "content")?)? {
                let content = object(content)?;
                if field_text(content, "type", 128)? == "text" {
                    field_text(content, "text", MAX_CONTENT_BYTES)?;
                }
            }
        }
        "reasoning" => {
            for k in ["summary", "content"] {
                if let Some(v) = i.get(k) {
                    for s in array(v)? {
                        text(s, MAX_CONTENT_BYTES)?;
                    }
                }
            }
        }
        "commandExecution" => {
            field_text(i, "command", MAX_CONTENT_BYTES)?;
            field_text(i, "cwd", 2048)?;
            array(required(i, "commandActions")?)?;
            one_of(
                required(i, "status")?,
                &["inProgress", "completed", "failed", "declined"],
            )?;
            if let Some(v) = i.get("exitCode").filter(|v| !v.is_null()) {
                let n = integer(v)?;
                if i32::try_from(n).is_err() {
                    return fail("invalid_exit_code");
                }
            }
            optional_text(i, "aggregatedOutput", MAX_CONTENT_BYTES)?;
        }
        "fileChange" => {
            one_of(
                required(i, "status")?,
                &["inProgress", "completed", "failed", "declined"],
            )?;
            for c in array(required(i, "changes")?)? {
                let c = object(c)?;
                field_text(c, "path", 2048)?;
                field_text(c, "diff", MAX_CONTENT_BYTES)?;
                let k = object(required(c, "kind")?)?;
                one_of(required(k, "type")?, &["add", "delete", "update"])?;
                optional_text(k, "move_path", 2048)?;
            }
        }
        "mcpToolCall" => {
            for k in ["server", "tool"] {
                field_text(i, k, 256)?;
            }
            required(i, "arguments")?;
            one_of(
                required(i, "status")?,
                &["inProgress", "completed", "failed"],
            )?;
        }
        _ => (),
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    Command,
    FileChange,
    Permissions,
}
pub fn approval_kind(method: &str) -> Option<ApprovalKind> {
    match method {
        "item/commandExecution/requestApproval" => Some(ApprovalKind::Command),
        "item/fileChange/requestApproval" => Some(ApprovalKind::FileChange),
        "item/permissions/requestApproval" => Some(ApprovalKind::Permissions),
        _ => None,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkAction {
    Allow,
    Deny,
}
#[derive(Debug, Clone)]
pub enum ApprovalDecision {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
    AcceptWithExecpolicyAmendment(Vec<String>),
    ApplyNetworkPolicyAmendment { action: NetworkAction, host: String },
}
impl ApprovalDecision {
    fn value(&self) -> Value {
        match self {
            Self::Accept => json!("accept"),
            Self::AcceptForSession => json!("acceptForSession"),
            Self::Decline => json!("decline"),
            Self::Cancel => json!("cancel"),
            Self::AcceptWithExecpolicyAmendment(v) => {
                json!({"acceptWithExecpolicyAmendment":{"execpolicy_amendment":v}})
            }
            Self::ApplyNetworkPolicyAmendment { action, host } => {
                json!({"applyNetworkPolicyAmendment":{"network_policy_amendment":{"action":action,"host":host}}})
            }
        }
    }
}
pub fn encode_approval_response(
    request: &WireMessage,
    decision: ApprovalDecision,
) -> Result<WireMessage> {
    if request.kind() != WireKind::ServerRequest {
        return fail("not_approval_request");
    }
    let kind = approval_kind(request.method().unwrap()).ok_or(CodecError {
        code: "unsupported_server_request",
    })?;
    if kind == ApprovalKind::Permissions {
        return fail("unsupported_decision");
    }
    let operation = if kind == ApprovalKind::Command {
        Operation::CommandApproval
    } else {
        Operation::FileApproval
    };
    let result = json!({"decision":decision.value()});
    validate_result(operation, &result)?;
    WireMessage::from_value(
        json!({"id":request.id(),"result":result}),
        Direction::ClientToServer,
    )
}
/// Only the verified empty-grant permission response is implemented in 2B.
/// Nonempty grants require a future authority decision and additional codecs.
pub fn encode_empty_permission_response(request: &WireMessage) -> Result<WireMessage> {
    if request.kind() != WireKind::ServerRequest
        || approval_kind(request.method().unwrap()) != Some(ApprovalKind::Permissions)
    {
        return fail("not_approval_request");
    }
    WireMessage::from_value(
        json!({"id":request.id(),"result":{"permissions":{},"scope":"turn"}}),
        Direction::ClientToServer,
    )
}
fn permission_profile(v: &Value) -> Result<()> {
    let p = object(v)?;
    only(p, &["fileSystem", "network"])?;
    if let Some(n) = p.get("network").filter(|v| !v.is_null()) {
        let n = object(n)?;
        optional_bool(n, "enabled")?;
    }
    if let Some(f) = p.get("fileSystem").filter(|v| !v.is_null()) {
        let f = object(f)?;
        for k in ["read", "write"] {
            if let Some(v) = f.get(k).filter(|v| !v.is_null()) {
                for path in array(v)? {
                    text(path, 2048)?;
                }
            }
        }
        if let Some(v) = f.get("entries").filter(|v| !v.is_null()) {
            for entry in array(v)? {
                object(entry)?;
            }
        }
        if let Some(v) = f.get("globScanMaxDepth").filter(|v| !v.is_null())
            && !v.as_u64().is_some_and(|n| n > 0)
        {
            return fail("invalid_depth");
        }
    }
    Ok(())
}
fn decision(v: &Value, command: bool) -> Result<()> {
    if v.is_string() {
        return one_of(v, &["accept", "acceptForSession", "decline", "cancel"]);
    }
    if !command {
        return fail("invalid_decision");
    }
    let d = object(v)?;
    if d.len() != 1 {
        return fail("invalid_decision");
    }
    if let Some(v) = d.get("acceptWithExecpolicyAmendment") {
        let p = object(v)?;
        only(p, &["execpolicy_amendment"])?;
        for s in array(required(p, "execpolicy_amendment")?)? {
            text(s, 2048)?;
        }
    } else if let Some(v) = d.get("applyNetworkPolicyAmendment") {
        let p = object(v)?;
        only(p, &["network_policy_amendment"])?;
        let n = object(required(p, "network_policy_amendment")?)?;
        only(n, &["action", "host"])?;
        one_of(required(n, "action")?, &["allow", "deny"])?;
        field_text(n, "host", 2048)?;
    } else {
        return fail("invalid_decision");
    }
    Ok(())
}

/// Correlation context is supplied by the caller, never guessed from response data.
pub fn validate_response(
    message: &WireMessage,
    id: &CodexRequestId,
    operation: Operation,
) -> Result<()> {
    let expected_direction = if matches!(
        operation,
        Operation::CommandApproval | Operation::FileApproval | Operation::PermissionApproval
    ) {
        Direction::ClientToServer
    } else {
        Direction::ServerToClient
    };
    if message.direction() != expected_direction || message.id() != Some(id) {
        return fail("response_correlation");
    }
    match message.kind() {
        WireKind::ErrorResponse => Ok(()),
        WireKind::SuccessResponse => validate_result(operation, message.result().unwrap()),
        _ => fail("not_response"),
    }
}
pub(crate) fn validate_result(operation: Operation, v: &Value) -> Result<()> {
    let p = object(v)?;
    match operation {
        Operation::Initialize => {
            for k in ["userAgent", "codexHome", "platformFamily", "platformOs"] {
                field_text(p, k, 2048)?;
            }
        }
        Operation::ThreadStart | Operation::ThreadResume => {
            for k in ["cwd", "model", "modelProvider"] {
                field_text(p, k, 2048)?;
            }
            let policy = required(p, "approvalPolicy")?;
            if !policy.is_string() && !policy.is_object() {
                return fail("invalid_approval_policy");
            }
            field_text(p, "approvalsReviewer", 128)?;
            let sandbox = object(required(p, "sandbox")?)?;
            field_text(sandbox, "type", 128)?;
            validate_thread(required(p, "thread")?)?;
        }
        Operation::TurnStart => validate_turn(required(p, "turn")?)?,
        Operation::ThreadArchive => only(p, &[])?,
        Operation::ThreadUnarchive => {
            only(p, &["thread"])?;
            validate_thread(required(p, "thread")?)?;
        }
        Operation::TurnInterrupt => only(p, &[])?,
        Operation::ModelList => {
            optional_text(p, "nextCursor", 2048)?;
            for model in array(required(p, "data")?)? {
                let m = object(model)?;
                for k in [
                    "id",
                    "model",
                    "displayName",
                    "description",
                    "defaultReasoningEffort",
                ] {
                    field_text(m, k, if k == "description" { 2048 } else { 256 })?;
                }
                if m["defaultReasoningEffort"].as_str().unwrap().is_empty() {
                    return fail("invalid_effort");
                }
                for k in ["hidden", "isDefault"] {
                    boolean(required(m, k)?)?;
                }
                for e in array(required(m, "supportedReasoningEfforts")?)? {
                    let e = object(e)?;
                    if field_text(e, "reasoningEffort", 256)?.is_empty() {
                        return fail("invalid_effort");
                    }
                    field_text(e, "description", 2048)?;
                }
                if let Some(v) = m.get("inputModalities") {
                    for v in array(v)? {
                        one_of(v, &["text", "image"])?;
                    }
                }
            }
        }
        Operation::CommandApproval | Operation::FileApproval => {
            only(p, &["decision"])?;
            decision(
                required(p, "decision")?,
                operation == Operation::CommandApproval,
            )?;
        }
        Operation::PermissionApproval => {
            // Deliberately bounded implemented subset, not a permission-policy engine.
            only(p, &["permissions", "scope", "strictAutoReview"])?;
            if !object(required(p, "permissions")?)?.is_empty() {
                return fail("unsupported_decision");
            }
            if let Some(v) = p.get("scope") {
                one_of(v, &["turn", "session"])?;
            }
            optional_bool(p, "strictAutoReview")?;
        }
    }
    Ok(())
}

/// Sprint 2G production surface. No model/config/instructions or turn input can be encoded.
pub(crate) fn managed_thread_request(
    id: CodexRequestId,
    cwd: &str,
    thread: Option<&CodexThreadId>,
) -> Result<WireMessage> {
    let (method, params) = match thread {
        None => ("thread/start", json!({"cwd":cwd,"ephemeral":false})),
        Some(id) => (
            "thread/resume",
            json!({"threadId":id,"cwd":cwd,"excludeTurns":true}),
        ),
    };
    WireMessage::parse(
        &serde_json::to_vec(&json!({"id":id,"method":method,"params":params})).map_err(|_| {
            CodecError {
                code: "encode_failed",
            }
        })?,
        Direction::ClientToServer,
    )
}

/// Internal empty-thread persistence barrier; no generic method or parameter surface.
pub(crate) fn materialization_request(
    id: CodexRequestId,
    thread: &CodexThreadId,
    unarchive: bool,
) -> Result<WireMessage> {
    WireMessage::request(
        id,
        if unarchive {
            "thread/unarchive"
        } else {
            "thread/archive"
        },
        json!({"threadId":thread}),
    )
}
