use crate::safety::classifier::{classify_tool_call, operation_human_description, RiskLevel};
use crate::safety::plan::PlanApprovalState;
use crate::safety::scope::ScopeGuard;
use crate::safety::undo::{self, UndoStack};
use crate::connectors::{self, ConnectorRegistry};
use crate::tools;
use crate::types::AgentEvent;
use crate::InterruptState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use tauri::{Emitter, State};

fn current_os_label() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macOS"
    }
    #[cfg(target_os = "linux")]
    {
        "Linux"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "this operating system"
    }
}

fn tool_category(name: &str) -> &'static str {
    if name.starts_with("browser_") {
        "browser"
    } else if connectors::is_connector_tool(name) {
        "connector"
    } else {
        match name {
            "read_file" | "list_dir" | "create_dir" | "move_file" | "delete_file" | "open_path" => "filesystem",
            "shell_exec" => "shell",
            "keyboard_type" | "keyboard_hotkey" => "keyboard",
            "screenshot" | "computer_use" => "vision",
            _ => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComputerUseAction {
    action: String,
    x: Option<i32>,
    y: Option<i32>,
    text: Option<String>,
    keys: Option<Vec<String>>,
    key: Option<String>,
    direction: Option<String>,
    amount: Option<i32>,
    reason: Option<String>,
}

fn read_runtime_env(name: &str) -> Option<String> {
    if let Ok(v) = std::env::var(name) {
        let trimmed = v.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    for path in [".env", "../.env"] {
        if let Ok(contents) = std::fs::read_to_string(path) {
            for line in contents.lines() {
                let line = line.trim();
                if line.starts_with('#') || !line.contains('=') {
                    continue;
                }
                if let Some(rest) = line.strip_prefix(&format!("{}=", name)) {
                    let value = rest.trim().trim_matches('"').trim_matches('\'');
                    if !value.is_empty() {
                        return Some(value.to_string());
                    }
                }
            }
        }
    }

    None
}

fn strip_markdown_fences(raw: &str) -> String {
    let trimmed = raw.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let mut lines = trimmed.lines();
    let _opening = lines.next();
    let mut body: Vec<&str> = Vec::new();
    for line in lines {
        if line.trim_start().starts_with("```") {
            break;
        }
        body.push(line);
    }
    body.join("\n").trim().to_string()
}

fn parse_key_combo(combo: &str) -> Vec<String> {
    combo
        .split('+')
        .map(|k| k.trim().to_lowercase())
        .filter(|k| !k.is_empty())
        .collect()
}

async fn request_openrouter_action(
    client: &reqwest::Client,
    api_key: &str,
    task: &str,
    screenshot_b64: &str,
    width: u32,
    height: u32,
    history: &[Value],
) -> Result<ComputerUseAction, String> {
    let history_json = serde_json::to_string_pretty(history)
        .map_err(|e| format!("Failed to serialize action history: {}", e))?;

    let instruction_text = format!(
        "You are a desktop control planner.\n\
         Decide exactly ONE next action based on the screenshot.\n\
         Return ONLY a JSON object with no markdown.\n\
         Allowed actions: click, double_click, right_click, type, scroll, key, done, fail.\n\
         Requirements:\n\
         - For click/double_click/right_click include integer x and y (screen coordinates).\n\
         - For type include text.\n\
         - For key include either keys (array) or key (string combo like ctrl+l).\n\
         - For scroll include direction (up/down) and amount integer.\n\
         - For done/fail include reason.\n\
         Current screen resolution: {width}x{height}.\n\
         User task: {task}\n\
         Previous actions:\n{history_json}\n"
    );

    let payload = json!({
        "model": "qwen/qwen2.5-vl-72b-instruct",
        "messages": [
            {
                "role": "system",
                "content": "Return strictly valid JSON for one action. No prose. No markdown fences."
            },
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": instruction_text },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/png;base64,{}", screenshot_b64)
                        }
                    }
                ]
            }
        ]
    });

    let response = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(api_key)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("OpenRouter request failed: {}", e))?;

    let status = response.status();
    let data: Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid OpenRouter response JSON: {}", e))?;

    if !status.is_success() {
        let msg = data
            .get("error")
            .and_then(|v| v.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("OpenRouter API returned error")
            .to_string();
        return Err(format!("OpenRouter error ({}): {}", status, msg));
    }

    let raw_content = data["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "OpenRouter response missing choices[0].message.content".to_string())?;

    let cleaned = strip_markdown_fences(raw_content);
    serde_json::from_str::<ComputerUseAction>(&cleaned)
        .map_err(|e| format!("Failed to parse action JSON from model output: {}", e))
}

async fn run_computer_use_loop(
    task: &str,
    max_steps: usize,
    window: &tauri::Window,
    interrupt_state: &State<'_, InterruptState>,
) -> Result<String, String> {
    let api_key = read_runtime_env("OPENROUTER_KEY")
        .ok_or_else(|| "OPENROUTER_KEY was not found in runtime env or .env".to_string())?;

    let client = reqwest::Client::new();
    let mut history: Vec<Value> = Vec::new();
    let max_steps = max_steps.max(1);

    for step in 1..=max_steps {
        if interrupt_state.0.load(Ordering::SeqCst) {
            return Err("Computer-use loop interrupted by user.".to_string());
        }

        window.hide().ok();
        tokio::time::sleep(std::time::Duration::from_millis(180)).await;
        let frame = tools::screenshot_frame_tool();
        window.show().ok();
        let frame = frame?;

        let action = request_openrouter_action(
            &client,
            &api_key,
            task,
            &frame.base64_png,
            frame.width,
            frame.height,
            &history,
        )
        .await?;

        let normalized = action.action.trim().to_lowercase();
        let exec_result = match normalized.as_str() {
            "click" => {
                let x = action.x.ok_or_else(|| "click action missing x".to_string())?;
                let y = action.y.ok_or_else(|| "click action missing y".to_string())?;
                tools::mouse_move_tool(x, y)?;
                tools::mouse_click_tool("left", "single")?
            }
            "double_click" => {
                let x = action.x.ok_or_else(|| "double_click action missing x".to_string())?;
                let y = action.y.ok_or_else(|| "double_click action missing y".to_string())?;
                tools::mouse_move_tool(x, y)?;
                tools::mouse_click_tool("left", "double")?
            }
            "right_click" => {
                let x = action.x.ok_or_else(|| "right_click action missing x".to_string())?;
                let y = action.y.ok_or_else(|| "right_click action missing y".to_string())?;
                tools::mouse_move_tool(x, y)?;
                tools::mouse_click_tool("right", "single")?
            }
            "type" => {
                let text = action
                    .text
                    .clone()
                    .ok_or_else(|| "type action missing text".to_string())?;
                tools::keyboard_type_tool(&text)?
            }
            "scroll" => {
                let direction = action.direction.as_deref().unwrap_or("down");
                let amount = action.amount.unwrap_or(3).max(1);
                tools::mouse_scroll_tool(direction, amount)?
            }
            "key" => {
                let keys = if let Some(arr) = action.keys.clone() {
                    arr
                } else if let Some(combo) = action.key.clone() {
                    parse_key_combo(&combo)
                } else {
                    Vec::new()
                };
                if keys.is_empty() {
                    return Err("key action missing keys/key field".to_string());
                }
                tools::keyboard_hotkey_tool(&keys)?
            }
            "done" => {
                let reason = action.reason.unwrap_or_else(|| "Task completed".to_string());
                history.push(json!({
                    "step": step,
                    "action": "done",
                    "result": reason,
                }));
                return Ok(json!({
                    "status": "done",
                    "steps": step,
                    "resolution": format!("{}x{}", frame.width, frame.height),
                    "history": history
                })
                .to_string());
            }
            "fail" => {
                let reason = action.reason.unwrap_or_else(|| "Model reported fail".to_string());
                history.push(json!({
                    "step": step,
                    "action": "fail",
                    "result": reason,
                }));
                return Err(json!({
                    "status": "fail",
                    "steps": step,
                    "resolution": format!("{}x{}", frame.width, frame.height),
                    "history": history
                })
                .to_string());
            }
            other => {
                return Err(format!("Unsupported action from vision model: {}", other));
            }
        };

        history.push(json!({
            "step": step,
            "action": action,
            "result": exec_result,
            "resolution": format!("{}x{}", frame.width, frame.height),
        }));

        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    }

    Err(json!({
        "status": "max_steps_reached",
        "steps": max_steps,
        "history": history
    })
    .to_string())
}

fn task_switch_reason(previous_category: &str, next_category: &str) -> &'static str {
    match (previous_category, next_category) {
        ("shell", "browser") => "The task moved from local shell automation to web interaction, so browser tools are required.",
        ("browser", "filesystem") => "The task moved from web context to local files, so file-system tools are required.",
        ("filesystem", "browser") => "The task moved from local files to a web flow, so browser tools are required.",
        (_, "vision") => "A visual check is needed for this step, so screenshot-based reasoning is required.",
        ("vision", _) => "After visual inspection, the model is switching back to an action tool to continue execution.",
        _ => "The next step uses a different capability than the previous one to complete the request correctly.",
    }
}

fn build_system_prompt() -> String {
    let os = current_os_label();
    let applescript_note = if cfg!(target_os = "macos") {
        "- run_applescript(script): Available on macOS for app automation. Great for opening apps, controlling system UI.\n"
    } else {
        ""
    };

    format!(
        "You are Imprint, an expert desktop automation agent running on {os}.\n\
         Tailor all actions and wording for {os}.\n\
         If the user greets you, mention {os} (not any other OS).\n\n\
         TOOL PRIORITY ORDER (highest to lowest):\n\n\
         TIER 0 — CONNECTORS / MCPS (use these before browser or GUI automation):\n\
         - Use Google and GitHub connector tools for Gmail, Calendar, repository, issue, and pull-request work.\n\
         - Reads/searches can run directly. Writes such as sending email, creating calendar events, creating issues, or commenting require user approval.\n\
         - Prefer connector tools over browser automation whenever a connector can complete the action.\n\
         - Browser automation is fallback-only for web tasks without a connector tool.\n\n\
         TIER 1 — FILE & SYSTEM TOOLS (use these first, they are fast and reliable):\n\
         - shell_exec(command): Run shell commands. Best for opening apps, running scripts, file ops, querying system.\n\
         - open_path(path): Open a folder/file in the native file manager.\n\
         - read_file(path, max_chars): Read file contents.\n\
         - list_dir(path): List directory contents as JSON.\n\
         - create_dir(path): Create a directory including parent directories.\n\
         - move_file(from, to): Move or rename a file or folder.\n\
         - delete_file(path): Delete a file or folder permanently.\n\
         {applescript_note}\n\
         TIER 2 — KEYBOARD & HOTKEYS (use when shell isn't suitable, e.g. interacting with focused windows):\n\
         - keyboard_hotkey(keys): Press a key combination (e.g. [\"ctrl\", \"c\"], [\"win\", \"i\"], [\"alt\", \"tab\"]).\n\
         - keyboard_type(text): Type a text string.\n\n\
         TIER 2.5 — BROWSER AUTOMATION (fallback only for web tasks with no connector):\n\
         - browser_navigate(url): Open a URL in Chrome. Connects automatically.\n\
         - browser_get_page_state(): Get interactive elements (numbered) + annotated screenshot. ALWAYS call this first on a new page.\n\
         - browser_click(selector): Click by ELEMENT INDEX NUMBER (preferred, e.g. \"7\"), accessible name, or CSS selector.\n\
         - browser_type_text(selector, text): Type into a field by index number, name, or CSS selector.\n\
         - browser_scroll(direction, amount): Scroll the page up or down.\n\
         - browser_extract_text(selector?): Get text content from page or element.\n\
         - browser_screenshot(): Capture browser viewport.\n\
         - browser_wait_for(selector, timeout_ms?): Wait for an element to appear.\n\
         - browser_go_back(): Navigate back in history.\n\
         - browser_evaluate(expression): Run JavaScript on the page (advanced).\n\
         APPROACH: Navigate first, then call browser_get_page_state. The screenshot will have orange numbered labels on interactive elements. Use those numbers with browser_click (e.g. browser_click(selector=\"7\")) — this is the MOST RELIABLE method. You can also use element names from the elements list.\n\
         CRITICAL BROWSER RULES:\n\
         - For web tasks with a matching connector, DO NOT use browser tools. Use the connector tool.\n\
         - For browser fallback tasks, call browser_navigate as the VERY FIRST browser tool call.\n\
         - NEVER use shell_exec to open, launch, restart, or kill Chrome. Not 'open -a Google Chrome', not pkill, not killall. Nothing.\n\
         - Chrome lifecycle (launch, restart, session restore) is handled AUTOMATICALLY by the browser tools. You do nothing.\n\
         - If a browser tool fails after 2 retries, report the error text to the user verbatim and stop. Do not try shell workarounds.\n\
         - The 'PRIORITIZE TERMINAL' rule does NOT apply to browser tasks. For browser fallback tasks, use browser_* tools only.\n\n\
         TIER 3 — VISION / MOUSE CONTROL (for GUI interactions that require clicking on screen elements):\n\
         - screenshot(): Capture the screen as an image for visual analysis. Returns a base64 PNG image.\n\
         - mouse_move(x, y): Move the mouse cursor to absolute screen coordinates. ALWAYS call this before mouse_click.\n\
         - mouse_click(button?, click_type?): Click at the CURRENT cursor position. Options: button=left/right/middle, click_type=single/double.\n\
         - mouse_scroll(direction, amount?): Scroll at the current cursor position. Directions: up/down/left/right.\n\
         - computer_use(task, max_steps?): Iterative screen-control loop using vision actions (click/type/scroll/key) until done/fail. Use for complex multi-step GUI workflows.\n\
         MOUSE CLICK WORKFLOW — always follow this exact order:\n\
           1. Call screenshot() to see the screen and identify target coordinates.\n\
           2. Call mouse_move(x=<X>, y=<Y>) with the pixel coordinates of the target element.\n\
           3. Call mouse_click(button=\"left\", click_type=\"single\") to click.\n\
           For right-click: same flow but mouse_click(button=\"right\", click_type=\"single\").\n\
         COORDINATE NOTE: Coordinates from screenshot() are in the resized image space (max 1280px wide).\n\
         The actual screen may be larger. Always use coordinates as shown in the screenshot without scaling.\n\
         Use screenshot() ONLY when:\n\
           a) You need to visually read information from the screen.\n\
           b) The task explicitly requires seeing/reading something on screen.\n\
           c) All other methods have failed and you need to visually verify the current state.\n\
         DO NOT use screenshot() just to confirm an action worked — trust the tool result output instead.\n\n\
         APPROACH FOR EVERY TASK:\n\
         1. If the request is unclear, ask ONE concise clarification question before acting.\n\
         2. For Gmail, Calendar, GitHub issues/PRs/repos, use connector tools before terminal, browser, or GUI automation.\n\
         3. PRIORITIZE TERMINAL OVER GUI for local/system tasks. Never use UI settings apps or simulated GUI inputs if a command-line equivalent exists. Exception: for web/browser fallback tasks, use browser_* tools exclusively — never shell_exec to manage Chrome processes.\n\
         4. Only resort to GUI interactions (screenshot, keyboard_type) if the task explicitly asks to interact with a visual element or if terminal/connector/browser methods are completely impossible.\n\
         5. Execute step-by-step and summarize exactly what changed.\n\
         6. Never claim actions for a different operating system.\n\
         7. Do not guess unknown app names/paths — ask before acting.\n\n\
         QUICK EXAMPLES:\n\
         - 'Open Calculator' → shell_exec('calc') on Windows, or shell_exec('open -a Calculator') on macOS.\n\
         - 'Search for a file' → shell_exec('dir /s filename' on Windows, 'find ~ -name filename' on macOS/Linux).\n\
         - 'Open Settings' → shell_exec('start ms-settings:') on Windows, keyboard_hotkey(['win', 'i']).\n\
         - 'Copy text in focused window' → keyboard_hotkey(['ctrl', 'c']).\n\
         - 'What is currently on screen?' → screenshot() [this is a valid use of vision].\n\n\
         SAFETY:\n\
         All tool calls are risk-classified before execution.\n\
         Destructive operations are logged with undo snapshots.\n\
         Operations outside the allowed path scope are blocked.\n\
         Sudo and privilege escalation are blocked by default.\n\
         Dangerous hotkeys (Ctrl+Alt+Del, Alt+F4) require explicit user approval."
    )
}


fn build_tools_declaration(connector_tools: Vec<Value>) -> Value {
    let mut function_declarations = vec![
        // ─── Computer Use Tools (highest priority) ───
        json!({
            "name": "screenshot",
            "description": "LAST RESORT: Capture the screen as an image for visual analysis. Only use when: (1) you need pixel coordinates of a UI element and no other method works, (2) the task explicitly asks what is on screen, or (3) Tier 1 and Tier 2 tools have failed. Do NOT use to verify that a command worked — trust the tool result text instead.",
            "parameters": {
                "type": "OBJECT",
                "properties": {}
            }
        }),
        json!({
            "name": "mouse_move",
            "description": "Move the mouse cursor to absolute screen coordinates (x, y). Use integer pixel coordinates identified from screenshot analysis. Always call this before mouse_click.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "x": { "type": "INTEGER", "description": "X coordinate on screen (pixels from left edge)" },
                    "y": { "type": "INTEGER", "description": "Y coordinate on screen (pixels from top edge)" }
                },
                "required": ["x", "y"]
            }
        }),
        json!({
            "name": "mouse_click",
            "description": "Click the mouse at the CURRENT cursor position. Always call mouse_move first to position the cursor, then call this. Works for left click, right click, and double click.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "button": { "type": "STRING", "description": "Mouse button: left, right, or middle (default: left)" },
                    "click_type": { "type": "STRING", "description": "Click type: single or double (default: single)" }
                }
            }
        }),
        json!({
            "name": "keyboard_type",
            "description": "Type a text string using the keyboard. Characters are typed sequentially.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "text": { "type": "STRING", "description": "The text string to type" }
                },
                "required": ["text"]
            }
        }),
        json!({
            "name": "keyboard_hotkey",
            "description": "Press a keyboard shortcut/hotkey combination. Keys are pressed together. Examples: [\"ctrl\", \"c\"], [\"win\"], [\"alt\", \"tab\"], [\"ctrl\", \"shift\", \"s\"]. Valid modifiers: ctrl, alt, shift, win/meta/super. Valid keys: enter, tab, escape, backspace, delete, space, up, down, left, right, home, end, pageup, pagedown, f1-f12, or any single character.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "keys": {
                        "type": "ARRAY",
                        "items": { "type": "STRING" },
                        "description": "Array of key names to press simultaneously"
                    }
                },
                "required": ["keys"]
            }
        }),
        json!({
            "name": "computer_use",
            "description": "Run an iterative vision-action loop on the desktop. At each step: capture screenshot, ask vision model for one JSON action (click/double_click/right_click/type/scroll/key/done/fail), execute the action, then repeat until done/fail/max steps.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "task": { "type": "STRING", "description": "The user's UI task goal" },
                    "max_steps": { "type": "INTEGER", "description": "Maximum loop iterations before stopping (default: 15)" }
                },
                "required": ["task"]
            }
        }),
        json!({
            "name": "mouse_scroll",
            "description": "Scroll the mouse wheel at the current cursor position. Move the mouse to the target area first with mouse_move.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "direction": { "type": "STRING", "description": "Scroll direction: up, down, left, or right" },
                    "amount": { "type": "INTEGER", "description": "Number of scroll clicks (default: 3)" }
                },
                "required": ["direction"]
            }
        }),
        // ─── File & System Tools ───
        json!({
            "name": "open_path",
            "description": "Open a folder/file path in the native OS file manager.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "path": { "type": "STRING", "description": "Absolute or ~ path to open" }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "list_dir",
            "description": "List the contents of a directory. Call this before file operations.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "path": { "type": "STRING", "description": "Absolute or ~ path to list" }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "create_dir",
            "description": "Create a directory including all parent directories.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "path": { "type": "STRING", "description": "Absolute or ~ path to create" }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "move_file",
            "description": "Move or rename a file or folder.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "from": { "type": "STRING", "description": "Source path" },
                    "to": { "type": "STRING", "description": "Destination path" }
                },
                "required": ["from", "to"]
            }
        }),
        json!({
            "name": "delete_file",
            "description": "Delete a file or folder permanently.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "path": { "type": "STRING", "description": "Absolute or ~ path to delete" }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "shell_exec",
            "description": "Run shell commands appropriate for the current OS (PowerShell on Windows, Bash on macOS/Linux). This is the PRIMARY and PREFERRED tool for accomplishing virtually any task. Use it to configure settings, open apps, fetch data, and automate actions. Always attempt to solve the user's request using this tool via command-line BEFORE trying to simulate keyboard GUI interactions.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "command": { "type": "STRING", "description": "The shell command to execute" }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "read_file",
            "description": "Read the text contents of a file.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "path": { "type": "STRING", "description": "Absolute or ~ path to the file" },
                    "max_chars": { "type": "INTEGER", "description": "Maximum characters to return (default 2000)" }
                },
                "required": ["path"]
            }
        }),
    ];

    if cfg!(target_os = "macos") {
        function_declarations.push(json!({
            "name": "run_applescript",
            "description": "Run AppleScript for macOS app automation.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "script": { "type": "STRING", "description": "The complete AppleScript to execute" }
                },
                "required": ["script"]
            }
        }));
    }

    // ─── Browser Automation Tools ───
    function_declarations.push(json!({
        "name": "browser_navigate",
        "description": "Navigate the browser to a URL. Connects to Chrome automatically on first use. Use this to open web pages for research, form filling, or web automation.",
        "parameters": {
            "type": "OBJECT",
            "properties": {
                "url": { "type": "STRING", "description": "The URL to navigate to (e.g. https://example.com)" }
            },
            "required": ["url"]
        }
    }));
    function_declarations.push(json!({
        "name": "browser_click",
        "description": "Click an element on the page. PREFERRED: Use the element index number from browser_get_page_state (e.g. \"7\" for element labeled 7 in the screenshot). Also accepts element name or CSS selector as fallback.",
        "parameters": {
            "type": "OBJECT",
            "properties": {
                "selector": { "type": "STRING", "description": "Element index number (e.g. \"7\"), accessible element name (e.g. \"Search\"), or CSS selector as fallback" }
            },
            "required": ["selector"]
        }
    }));
    function_declarations.push(json!({
        "name": "browser_type_text",
        "description": "Type text into an input field. Use element index number from browser_get_page_state (preferred), element name, or CSS selector to identify the field.",
        "parameters": {
            "type": "OBJECT",
            "properties": {
                "selector": { "type": "STRING", "description": "Element index number (e.g. \"12\"), accessible element name, or CSS selector" },
                "text": { "type": "STRING", "description": "The text to type into the field" }
            },
            "required": ["selector", "text"]
        }
    }));
    function_declarations.push(json!({
        "name": "browser_scroll",
        "description": "Scroll the page up or down by a pixel amount.",
        "parameters": {
            "type": "OBJECT",
            "properties": {
                "direction": { "type": "STRING", "description": "Scroll direction: 'up' or 'down'" },
                "amount": { "type": "NUMBER", "description": "Pixels to scroll (default: 500)" }
            },
            "required": ["direction"]
        }
    }));
    function_declarations.push(json!({
        "name": "browser_extract_text",
        "description": "Extract text content from the page or a specific element. Returns up to 4000 characters.",
        "parameters": {
            "type": "OBJECT",
            "properties": {
                "selector": { "type": "STRING", "description": "Optional CSS selector. If omitted, extracts all visible text from the page body." }
            }
        }
    }));
    function_declarations.push(json!({
        "name": "browser_screenshot",
        "description": "Take a screenshot of the current browser viewport. Returns a visual image for analysis.",
        "parameters": {
            "type": "OBJECT",
            "properties": {}
        }
    }));
    function_declarations.push(json!({
        "name": "browser_get_page_state",
        "description": "Get the current page state: URL, title, numbered list of interactive elements, and an annotated screenshot with orange index labels on each element. ALWAYS call this first on a new page. Use the element index numbers with browser_click and browser_type_text.",
        "parameters": {
            "type": "OBJECT",
            "properties": {}
        }
    }));
    function_declarations.push(json!({
        "name": "browser_wait_for",
        "description": "Wait for an element matching a CSS selector to appear on the page. Useful after navigation or actions that trigger dynamic content loading.",
        "parameters": {
            "type": "OBJECT",
            "properties": {
                "selector": { "type": "STRING", "description": "CSS selector to wait for" },
                "timeout_ms": { "type": "INTEGER", "description": "Maximum wait time in milliseconds (default: 30000)" }
            },
            "required": ["selector"]
        }
    }));
    function_declarations.push(json!({
        "name": "browser_go_back",
        "description": "Navigate back to the previous page in browser history.",
        "parameters": {
            "type": "OBJECT",
            "properties": {}
        }
    }));
    function_declarations.push(json!({
        "name": "browser_evaluate",
        "description": "Execute arbitrary JavaScript on the current page and return the result. Use for advanced interactions not covered by other browser tools.",
        "parameters": {
            "type": "OBJECT",
            "properties": {
                "expression": { "type": "STRING", "description": "JavaScript expression or statement to evaluate" }
            },
            "required": ["expression"]
        }
    }));

    function_declarations.extend(connector_tools);

    json!([{ "functionDeclarations": function_declarations }])
}

pub async fn run_agent(
    prompt: String,
    history: Vec<Value>,
    window: tauri::Window,
    interrupt_state: State<'_, InterruptState>,
    scope_guard: State<'_, ScopeGuard>,
    undo_stack: State<'_, UndoStack>,
    plan_state: State<'_, PlanApprovalState>,
    browser_state: State<'_, crate::browser::BrowserState>,
    connector_registry: State<'_, ConnectorRegistry>,
) -> Result<(), String> {
    let client = reqwest::Client::new();

    // API key is baked into the binary at compile time via build.rs reading the .env file.
    let api_key = env!("GEMINI_API_KEY");
    if api_key.is_empty() {
        return Err("GEMINI_API_KEY was not set at compile time. Add it to src-tauri/.env and rebuild.".to_string());
    }

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-flash-preview:generateContent?key={}",
        api_key
    );

    let system_prompt = build_system_prompt();
    let system_instruction = json!({
        "parts": [{ "text": system_prompt }]
    });

    let tools = build_tools_declaration(connectors::connector_tools_as_gemini_declarations());
    
    let mut contents = history;

    // If history is empty, add the initial user prompt.
    // If it's not empty, the caller should have already appended the message to the session,
    // and clui-shim will pass it in.
    if contents.is_empty() {
        contents.push(json!({ "role": "user", "parts": [{ "text": prompt }] }));
    } else {
        // If clui-shim is passing history correctly, the last message in history
        // is likely the current prompt. Let's verify or append as needed.
        // For simplicity with clui-shim's current logic, we'll assume clui-shim 
        // includes the current prompt in the history it sends.
    }
    // Clear undo stack at start of each run so the undo button always targets THIS run
    undo_stack.clear();

    let mut opened_paths: HashSet<String> = HashSet::new();
    let mut open_path_calls: usize = 0;
    let mut iteration_count: usize = 0;
    let mut previous_tool_context: Option<(String, String)> = None;

    // Loop detection state
    let mut previous_action_hash: Option<u64> = None;
    let mut identical_action_count: usize = 0;

    loop {
        iteration_count += 1;
        // Increased from 12 to 30 because Vision/UI tasks (screenshot -> move -> click -> type) eat up iterations very quickly!
        if iteration_count > 30 {
            let msg = "Stopped after too many steps. Please refine your request or approve a narrower action.".to_string();
            window
                .emit(
                    "agent_event",
                    AgentEvent {
                        kind: "message".into(),
                        content: msg,
                    },
                )
                .ok();
            window
                .emit(
                    "agent_event",
                    AgentEvent {
                        kind: "done".into(),
                        content: String::new(),
                    },
                )
                .ok();
            if let Some(manager) = browser_state.lock().await.as_mut() {
                manager.close_orphaned_tabs().await;
            }
            return Ok(());
        }

        if interrupt_state.0.load(Ordering::SeqCst) {
            let msg = "Agent interrupted by user.".to_string();
            window
                .emit(
                    "agent_event",
                    AgentEvent {
                        kind: "error".into(),
                        content: msg.clone(),
                    },
                )
                .ok();
            if let Some(manager) = browser_state.lock().await.as_mut() {
                manager.close_orphaned_tabs().await;
            }
            return Err(msg);
        }

        let body = json!({
            "systemInstruction": system_instruction,
            "tools": tools,
            "contents": contents,
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true
                }
            }
        });

        let response = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let data: Value = response.json().await.map_err(|e| e.to_string())?;

        // Check for API error
        if let Some(err) = data.get("error") {
            let msg = err["message"]
                .as_str()
                .unwrap_or("Gemini API error")
                .to_string();
            window
                .emit(
                    "agent_event",
                    AgentEvent {
                        kind: "error".into(),
                        content: msg.clone(),
                    },
                )
                .ok();
            if let Some(manager) = browser_state.lock().await.as_mut() {
                manager.close_orphaned_tabs().await;
            }
            return Err(msg);
        }

        let parts = data["candidates"][0]["content"]["parts"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        // Surface model thoughts to the UI when available.
        let thought_text = parts
            .iter()
            .filter_map(|p| {
                let is_thought = p
                    .get("thought")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if is_thought {
                    p.get("text").and_then(|v| v.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !thought_text.trim().is_empty() {
            window
                .emit(
                    "agent_event",
                    AgentEvent {
                        kind: "reasoning".into(),
                        content: thought_text,
                    },
                )
                .ok();
        }

        // Append model turn to contents
        contents.push(json!({
            "role": "model",
            "parts": parts
        }));

        // Check if any part is a functionCall
        let has_tool_calls = parts.iter().any(|p| p.get("functionCall").is_some());

        if has_tool_calls {
            use std::hash::{Hash, Hasher};
            use std::collections::hash_map::DefaultHasher;

            let mut current_action_summary = String::new();
            for part in &parts {
                if let Some(fc) = part.get("functionCall") {
                    current_action_summary.push_str(&fc.to_string());
                }
            }
            let mut hasher = DefaultHasher::new();
            current_action_summary.hash(&mut hasher);
            let current_hash = hasher.finish();

            if previous_action_hash == Some(current_hash) {
                identical_action_count += 1;
            } else {
                identical_action_count = 0;
                previous_action_hash = Some(current_hash);
            }

            if identical_action_count >= 3 {
                let msg = "Circuit breaker triggered: the exact same action was attempted 3 times without a meaningful state change. Please try a fundamentally different approach or ask the user for help. A new tab may have opened that cannot be reliably controlled.".to_string();
                window
                    .emit(
                        "agent_event",
                        AgentEvent {
                            kind: "error".into(),
                            content: msg.clone(),
                        },
                    )
                    .ok();
                
                // Return tool result informing model of failure
                contents.push(json!({
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "name": parts[0].get("functionCall").and_then(|fc| fc.get("name")).and_then(|n| n.as_str()).unwrap_or("unknown"),
                            "response": { "output": msg }
                        }
                    }]
                }));
                continue;
            }

            // Collect all function responses in one turn
            let mut function_responses: Vec<Value> = vec![];

            for part in &parts {
                if let Some(fc) = part.get("functionCall") {
                    let name = fc["name"].as_str().unwrap_or("");
                    let args = &fc["args"];

                    // Guardrail: avoid opening multiple file manager windows for a single request.
                    if name == "open_path" {
                        let requested = args["path"].as_str().unwrap_or("").trim().to_string();
                        if requested.is_empty() {
                            function_responses.push(json!({
                                "functionResponse": {
                                    "name": name,
                                    "response": { "output": "error: open_path requires a non-empty path" }
                                }
                            }));
                            continue;
                        }

                        if opened_paths.contains(&requested) {
                            function_responses.push(json!({
                                "functionResponse": {
                                    "name": name,
                                    "response": { "output": format!("skipped: path already opened in this request ({})", requested) }
                                }
                            }));
                            continue;
                        }

                        if open_path_calls >= 1 {
                            function_responses.push(json!({
                                "functionResponse": {
                                    "name": name,
                                    "response": { "output": "skipped: open_path already used once in this request" }
                                }
                            }));
                            continue;
                        }

                        opened_paths.insert(requested);
                        open_path_calls += 1;
                    }

                    // ── Safety: classify the tool call ──
                    let (operation, mut risk) = classify_tool_call(name, args);
                    if connectors::connector_tool_is_write(name) {
                        risk = RiskLevel::Dangerous;
                    }
                    // Screenshot approval is temporarily disabled.
                    // if name == "screenshot" {
                    //     risk = RiskLevel::Dangerous;
                    // }
                    let human_desc = operation_human_description(&operation);
                    let current_category = tool_category(name).to_string();

                    if let Some((prev_desc, prev_category)) = &previous_tool_context {
                        if prev_category != &current_category {
                            let payload = json!({
                                "previous_task": prev_desc,
                                "new_task": human_desc,
                                "reason": task_switch_reason(prev_category, &current_category),
                                "necessary": true
                            });
                            window
                                .emit(
                                    "agent_event",
                                    AgentEvent {
                                        kind: "task_switch".into(),
                                        content: payload.to_string(),
                                    },
                                )
                                .ok();
                        }
                    }

                    let mut screenshot_pre_result: Option<Result<String, String>> = None;
                    let mut preview_b64 = None;
                    if name == "screenshot" {
                        // Hide window before taking the screenshot so it doesn't obstruct the user's view
                        window.hide().ok();
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

                        let res = tools::screenshot_tool();

                        // Restore window
                        window.show().ok();
                        window.set_focus().ok();

                        if let Ok(ref s) = res {
                            if s.starts_with("SCREENSHOT_BASE64:") {
                                preview_b64 = Some(s["SCREENSHOT_BASE64:".len()..].to_string());
                            }
                        }
                        screenshot_pre_result = Some(res);
                    }

                    // ── Safety: scope check ──
                    let violation = scope_guard.check(&operation);
                    if violation.is_violation() {
                        let block_msg = violation.message();
                        window
                            .emit(
                                "agent_event",
                                AgentEvent {
                                    kind: "tool_call".into(),
                                    content: format!("[BLOCKED] {}: {}", name, block_msg),
                                },
                            )
                            .ok();
                        function_responses.push(json!({
                            "functionResponse": {
                                "name": name,
                                "response": { "output": format!("error: {}", block_msg) }
                            }
                        }));
                        continue;
                    }

                    // Emit tool_call event with risk badge
                    let risk_tag = match risk {
                        RiskLevel::Safe => "",
                        RiskLevel::Caution => " [⚠ caution]",
                        RiskLevel::Dangerous => " [🔴 dangerous]",
                        RiskLevel::Nuclear => " [☢ nuclear]",
                    };
                    let tool_call_payload = json!({
                        "name": name,
                        "args": args,
                        "human_desc": human_desc,
                        "risk_tag": risk_tag
                    });
                    window
                        .emit(
                            "agent_event",
                            AgentEvent {
                                kind: "tool_call".into(),
                                content: tool_call_payload.to_string(),
                            },
                        )
                        .ok();

                    // ── Safety: approval gate for Dangerous/Nuclear ops ──
                    if risk >= RiskLevel::Dangerous {
                        let plan_id = uuid::Uuid::new_v4().to_string();

                        // Build a minimal execution plan for the UI
                        let approval_payload = serde_json::json!({
                            "id": plan_id,
                            "summary": format!("The agent wants to: {}", human_desc),
                            "steps": [{
                                "index": 0,
                                "description": human_desc,
                                "operation": serde_json::to_value(&operation).unwrap_or(serde_json::json!({})),
                                "risk": risk.to_string(),
                                "can_undo": false,
                                "undo_description": null
                            }],
                            "overall_risk": risk.to_string(),
                            "preview_image": preview_b64
                        });

                        // Emit to frontend — PlanApprovalPanel listens for this
                        window.emit("plan_ready", &approval_payload).ok();

                        // Wait for user to approve or reject (5 min timeout)
                        let rx = plan_state.wait_for_approval(plan_id.clone());
                        let approved = match tokio::time::timeout(
                            std::time::Duration::from_secs(300),
                            rx,
                        )
                        .await
                        {
                            Ok(Ok(true)) => true,
                            Ok(Ok(false)) => false,
                            Ok(Err(_)) => false, // channel dropped
                            Err(_) => false,     // timeout
                        };

                        if !approved {
                            let reject_msg = format!("Operation cancelled by user: {}", human_desc);
                            window
                                .emit(
                                    "agent_event",
                                    AgentEvent {
                                        kind: "tool_result".into(),
                                        content: format!("CANCELLED: {}", reject_msg),
                                    },
                                )
                                .ok();
                            function_responses.push(json!({
                                "functionResponse": {
                                    "name": name,
                                    "response": { "output": format!("error: {}", reject_msg) }
                                }
                            }));
                            continue;
                        }
                    }

                    // ── Hard block: never allow shell_exec to manage Chrome ──
                    // Prompt-level rules are insufficient — LLMs deviate. Enforce in code.
                    if name == "shell_exec" {
                        let cmd = args["command"].as_str().unwrap_or("").to_lowercase();
                        let targets_chrome = cmd.contains("google chrome")
                            || cmd.contains("google-chrome")
                            || cmd.contains("chrome.exe");
                        let is_lifecycle = cmd.contains("open ")
                            || cmd.contains("start ")
                            || cmd.contains("pkill")
                            || cmd.contains("killall")
                            || cmd.contains("kill ")
                            || cmd.contains("launch");
                        if targets_chrome && is_lifecycle {
                            window
                                .emit(
                                    "agent_event",
                                    AgentEvent {
                                        kind: "tool_result".into(),
                                        content: "[BLOCKED] shell_exec targeting Chrome — use browser_navigate instead".into(),
                                    },
                                )
                                .ok();
                            function_responses.push(json!({
                                "functionResponse": {
                                    "name": name,
                                    "response": { "output": "error: Do not use shell commands to open or kill Chrome. Call browser_navigate directly — it manages Chrome automatically." }
                                }
                            }));
                            continue;
                        }
                    }

                    // ── Safety: snapshot before mutable ops ──
                    // Even RiskLevel::Safe operations like CreateDir should be snapshotted if they are mutable.
                    let snapshot = undo::snapshot_before_operation(&operation).ok();

                    // Execute the tool (no artificial delays — runs at full OS speed)
                    let result = if connectors::is_connector_tool(name) {
                        connector_registry.dispatch_tool(name, args).await
                    } else if name.starts_with("browser_") {
                        crate::browser::actions::dispatch_browser_tool(name, args, &browser_state).await
                    } else if name == "computer_use" {
                        let task = args["task"].as_str().unwrap_or("");
                        let max_steps = args["max_steps"].as_u64().unwrap_or(15) as usize;
                        run_computer_use_loop(task, max_steps, &window, &interrupt_state).await
                    } else if let Some(pre_res) = screenshot_pre_result {
                        pre_res
                    } else {
                        tools::dispatch_tool(name, args)
                    };

                    // ── Safety: push undo entry if we took a snapshot ──
                    if let Some(snap) = snapshot {
                        if result.is_ok() && snap.can_undo() {
                            undo_stack.push(undo::UndoEntry {
                                id: uuid::Uuid::new_v4().to_string(),
                                timestamp: Utc::now(),
                                description: human_desc.clone(),
                                snapshot: snap,
                            });
                        }
                    }

                    // ── Interrupt check: after each tool, bail immediately if requested ──
                    if interrupt_state.0.load(Ordering::SeqCst) {
                        window
                            .emit(
                                "agent_event",
                                AgentEvent {
                                    kind: "error".into(),
                                    content: "Agent interrupted by user.".into(),
                                },
                            )
                            .ok();
                        if let Some(manager) = browser_state.lock().await.as_mut() {
                            manager.close_orphaned_tabs().await;
                        }
                        return Err("Agent interrupted by user.".to_string());
                    }

                    // Emit tool_result event
                    match &result {
                        Ok(output) => window
                            .emit(
                                "agent_event",
                                AgentEvent {
                                    kind: "tool_result".into(),
                                    content: output.clone(),
                                },
                            )
                            .ok(),
                        Err(e) => window
                            .emit(
                                "agent_event",
                                AgentEvent {
                                    kind: "tool_result".into(),
                                    content: format!("ERROR: {}", e),
                                },
                            )
                            .ok(),
                    };

                    let output_str = result.unwrap_or_else(|e| format!("error: {}", e));
                    previous_tool_context = Some((human_desc.clone(), current_category));

                    // ── Vision: if the tool returned a screenshot, send as inline image ──
                    if output_str.starts_with("SCREENSHOT_BASE64:") {
                        // Pure screenshot (e.g. screenshot tool, browser_screenshot)
                        let b64_data = &output_str["SCREENSHOT_BASE64:".len()..];
                        function_responses.push(json!({
                            "functionResponse": {
                                "name": name,
                                "response": {
                                    "output": "Screenshot captured. Analyze the image to identify UI elements, their positions, and decide next actions."
                                }
                            }
                        }));
                        function_responses.push(json!({
                            "inlineData": {
                                "mimeType": "image/png",
                                "data": b64_data
                            }
                        }));
                    } else if let Some(idx) = output_str.find("\nSCREENSHOT_BASE64:") {
                        // Mixed output with embedded screenshot (e.g. browser_get_page_state)
                        let text_part = &output_str[..idx];
                        let b64_data = &output_str[idx + "\nSCREENSHOT_BASE64:".len()..];
                        function_responses.push(json!({
                            "functionResponse": {
                                "name": name,
                                "response": {
                                    "output": text_part
                                }
                            }
                        }));
                        function_responses.push(json!({
                            "inlineData": {
                                "mimeType": "image/png",
                                "data": b64_data
                            }
                        }));
                    } else {
                        function_responses.push(json!({
                            "functionResponse": {
                                "name": name,
                                "response": { "output": output_str }
                            }
                        }));
                    }
                }
            }

            // Append all tool results as a single user turn
            contents.push(json!({
                "role": "user",
                "parts": function_responses
            }));

            // Continue loop
        } else {
            // No tool calls — extract final text and finish
            let text = parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");

            window
                .emit(
                    "agent_event",
                    AgentEvent {
                        kind: "message".into(),
                        content: text,
                    },
                )
                .ok();

            window
                .emit(
                    "agent_event",
                    AgentEvent {
                        kind: "done".into(),
                        content: String::new(),
                    },
                )
                .ok();

            if let Some(manager) = browser_state.lock().await.as_mut() {
                manager.close_orphaned_tabs().await;
            }
            return Ok(());
        }
    }
}
