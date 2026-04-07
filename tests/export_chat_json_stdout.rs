use rusqlite::Connection;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

#[cfg(target_os = "linux")]
fn set_cursor_env(cmd: &mut Command, home: &Path) {
    cmd.env("HOME", home);
    cmd.env("XDG_CONFIG_HOME", home.join(".config"));
}

#[cfg(target_os = "macos")]
fn set_cursor_env(cmd: &mut Command, home: &Path) {
    cmd.env("HOME", home);
}

#[cfg(target_os = "windows")]
fn set_cursor_env(cmd: &mut Command, home: &Path) {
    cmd.env("USERPROFILE", home);
    cmd.env("APPDATA", home.join("AppData").join("Roaming"));
}

#[cfg(target_os = "linux")]
fn workspace_storage_root(home: &Path) -> std::path::PathBuf {
    home.join(".config")
        .join("Cursor")
        .join("User")
        .join("workspaceStorage")
}

#[cfg(target_os = "macos")]
fn workspace_storage_root(home: &Path) -> std::path::PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("Cursor")
        .join("User")
        .join("workspaceStorage")
}

#[cfg(target_os = "windows")]
fn workspace_storage_root(home: &Path) -> std::path::PathBuf {
    home.join("AppData")
        .join("Roaming")
        .join("Cursor")
        .join("User")
        .join("workspaceStorage")
}

#[cfg(target_os = "linux")]
fn global_storage_root(home: &Path) -> std::path::PathBuf {
    home.join(".config")
        .join("Cursor")
        .join("User")
        .join("globalStorage")
}

#[cfg(target_os = "macos")]
fn global_storage_root(home: &Path) -> std::path::PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("Cursor")
        .join("User")
        .join("globalStorage")
}

#[cfg(target_os = "windows")]
fn global_storage_root(home: &Path) -> std::path::PathBuf {
    home.join("AppData")
        .join("Roaming")
        .join("Cursor")
        .join("User")
        .join("globalStorage")
}

fn init_state_db(path: &Path) -> Connection {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }

    let conn = Connection::open(path).unwrap();
    conn.execute(
        "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )
    .unwrap();
    conn
}

fn setup_workspace(home: &Path, workspace_id: &str, workspace_uri: &str) {
    let workspace_root = workspace_storage_root(home);
    let workspace_dir = workspace_root.join(workspace_id);
    fs::create_dir_all(&workspace_dir).unwrap();
    fs::write(
        workspace_dir.join("workspace.json"),
        format!(r#"{{"folder":"{}"}}"#, workspace_uri),
    )
    .unwrap();

    let local_db = init_state_db(&workspace_dir.join("state.vscdb"));
    local_db
        .execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composer.composerData",
                r#"{"selectedComposerIds":["session-a"],"hasMigratedComposerData":true}"#
            ],
        )
        .unwrap();
}

fn setup_global_headers(home: &Path, headers_json: &str) -> Connection {
    let global_root = global_storage_root(home);
    let global_db = init_state_db(&global_root.join("state.vscdb"));
    global_db
        .execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params!["composer.composerHeaders", headers_json],
        )
        .unwrap();
    global_db
}

#[test]
fn export_chat_json_keeps_stdout_machine_readable() {
    let temp = TempDir::new().unwrap();
    let home = temp.path();

    let workspace_id = "workspace-123";
    setup_workspace(home, workspace_id, "file:///tmp/project");

    let global_db = setup_global_headers(
        home,
        &format!(
            r#"{{"allComposers":[{{"composerId":"session-a","name":"Visible","createdAt":1000,"lastUpdatedAt":2000,"isArchived":false,"workspaceIdentifier":{{"id":"{}","uri":{{"external":"file:///tmp/project","path":"/tmp/project"}}}}}},{{"composerId":"session-b","name":"Blank","createdAt":900,"lastUpdatedAt":1900,"isArchived":false,"workspaceIdentifier":{{"id":"{}","uri":{{"external":"file:///tmp/project","path":"/tmp/project"}}}}}}]}}"#,
            workspace_id, workspace_id
        ),
    );
    global_db
        .execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composerData:session-a",
                r#"{"fullConversationHeadersOnly":[{"bubbleId":"bubble-1","type":1}]}"#
            ],
        )
        .unwrap();
    global_db
        .execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composerData:session-b",
                r#"{"fullConversationHeadersOnly":[]}"#
            ],
        )
        .unwrap();
    global_db
        .execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:session-a:bubble-1",
                r#"{"text":"hello","createdAt":"2024-01-01T00:00:00.000Z"}"#
            ],
        )
        .unwrap();

    let binary = env!("CARGO_BIN_EXE_cursor-helper");
    let mut command = Command::new(binary);
    command.args([
        "export-chat",
        "--workspace-id",
        workspace_id,
        "--format",
        "json",
        "--exclude-blank",
    ]);
    set_cursor_env(&mut command, home);

    let output = command.output().unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    let export: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(export["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(export["sessions"][0]["id"], "session-a");
    assert!(stderr.contains("Filtered 1 blank session(s)"));
    assert!(stderr.contains("Found 1 chat session(s)"));
}

#[test]
fn export_chat_json_empty_result_still_prints_valid_json() {
    let temp = TempDir::new().unwrap();
    let home = temp.path();

    let workspace_id = "workspace-empty";
    setup_workspace(home, workspace_id, "file:///tmp/empty-project");

    let global_db = setup_global_headers(
        home,
        &format!(
            r#"{{"allComposers":[{{"composerId":"session-empty","name":"Blank","createdAt":1000,"lastUpdatedAt":2000,"isArchived":false,"workspaceIdentifier":{{"id":"{}","uri":{{"external":"file:///tmp/empty-project","path":"/tmp/empty-project"}}}}}}]}}"#,
            workspace_id
        ),
    );
    global_db
        .execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composerData:session-empty",
                r#"{"fullConversationHeadersOnly":[]}"#
            ],
        )
        .unwrap();

    let binary = env!("CARGO_BIN_EXE_cursor-helper");
    let mut command = Command::new(binary);
    command.args([
        "export-chat",
        "--workspace-id",
        workspace_id,
        "--format",
        "json",
        "--exclude-blank",
    ]);
    set_cursor_env(&mut command, home);

    let output = command.output().unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    let export: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(export["sessions"].as_array().unwrap().len(), 0);
    assert!(stderr.contains("Filtered 1 blank session(s)"));
    assert!(stderr.contains("No chat sessions found for this workspace."));
}
