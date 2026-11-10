use super::*;
use crate::hub::{Error as HubError, Workspace};
use sheprd_chat_protocol::{Agent, ENDPOINT_ENV, EndpointId, TOKEN};
use std::path::{Path, PathBuf};

/// A repository with one commit, named `project`, in `parent`.
fn committed_repository(parent: &Path) -> PathBuf {
    let repository = parent.join("project");
    std::fs::create_dir(&repository).unwrap();
    for args in [
        &["init", "--quiet"][..],
        &["commit", "--quiet", "--allow-empty", "-m", "start"],
    ] {
        let status = sheprd_client::new_command("git")
            .current_dir(&repository)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }
    repository
}

fn chat_record(address: &str, endpoint: &EndpointId) -> Value {
    let mut record = record(address);
    record["labels"] = json!({TOKEN: endpoint.to_string()});
    record
}

/// A hub whose Herdr 1 socket answers nothing, so reaching it fails with
/// `NotConnected` and a fallback cannot pass for success.
fn hub_on(source: &Arc<Source>, layout: Layout) -> Hub {
    let mut hub = Hub::offline_in(layout, None, None);
    hub.core = Some(source.clone());
    hub.program = Some(PathBuf::from("/bin/sheprd-acp"));
    hub
}

fn workspace(id: &str, cwd: &str) -> Workspace {
    Workspace {
        branch: None,
        chats: Vec::new(),
        cwd: cwd.to_owned(),
        id: id.to_owned(),
        label: id.to_owned(),
        terminals: Vec::new(),
        worktree: None,
        session: None,
    }
}

fn request(fake: &Fake) -> Value {
    let bytes = fake.inputs.recv_timeout(Duration::from_secs(2)).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn fake_with_chat(endpoint: &EndpointId) -> (Arc<Source>, Fake) {
    let records = vec![record("t1"), chat_record("t9", endpoint)];
    fake_with_doc(false, Value::Null, records)
}

#[test]
fn a_labelled_terminal_is_listed_as_a_chat_and_not_as_a_terminal() {
    let endpoint = EndpointId::random();
    let (source, fake) = fake_with_chat(&endpoint);
    let layout = source.poll().unwrap();
    let chat = layout.chat(&id("t9")).unwrap().1;
    assert_eq!(chat.endpoint, endpoint);
    assert_eq!(chat.agent.as_deref(), Some("claude"));
    assert!(layout.terminal(&id("t9")).is_none());
    assert!(layout.terminal(&id("t1")).is_some());
    drop(source);
    fake.server.join().unwrap();
}

#[test]
fn a_chat_on_another_machine_stays_a_terminal() {
    let endpoint = EndpointId::random();
    let records = vec![chat_record("box/t9", &endpoint)];
    let (source, fake) = fake_with_doc(false, Value::Null, records);
    let layout = source.poll().unwrap();
    assert!(layout.chat(&id("box/t9")).is_none());
    assert!(layout.terminal(&id("box/t9")).is_some());
    drop(source);
    fake.server.join().unwrap();
}

#[test]
fn merging_keeps_core_chats_in_a_workspace_both_sources_share() {
    let endpoint = EndpointId::random();
    let (source, fake) = fake_with_chat(&endpoint);
    let core = source.poll().unwrap();
    let daemon = Layout {
        workspaces: vec![workspace(UNLISTED, "/code")],
        ..Layout::default()
    };
    assert!(merge(&daemon, &core).chat(&id("t9")).is_some());
    drop(source);
    fake.server.join().unwrap();
}

#[tokio::test]
async fn a_chat_starts_on_the_core_with_its_endpoint_labelled() {
    let (source, fake) = fake_with_chat(&EndpointId::random());
    source.poll().unwrap();
    let layout = Layout {
        connected: true,
        workspaces: vec![workspace("w1", "/code/one")],
        ..Layout::default()
    };
    let hub = hub_on(&source, layout);
    let pane = hub.start_chat("w1", Agent::Claude, None).await.unwrap();
    assert_eq!(pane, id("t9"));
    let run = request(&fake);
    assert_eq!(run["method"], "terminal.run");
    let params = &run["params"];
    assert_eq!(params["argv"], json!(["/bin/sheprd-acp", "claude"]));
    assert_eq!(params["cwd"], "/code/one");
    assert_eq!(params["env"][ENDPOINT_ENV], params["labels"][TOKEN]);
    assert_eq!(params["env"]["HERDR_PANE_ID"], "");
    // Sheprd puts it in a tab of the workspace it was asked for.
    assert_eq!(params["labels"][sheprd_session::WORKSPACE_LABEL], "w1");
    drop((hub, source));
    fake.server.join().unwrap();
}

#[tokio::test]
async fn a_core_failure_is_reported_and_not_retried_on_herdr_one() {
    let (source, fake) = fake();
    source.poll().unwrap();
    let hub = hub_on(&source, Layout::default());
    let error = hub
        .create_workspace("/refused", None, Agent::Claude, None)
        .await
        .err()
        .unwrap();
    assert!(matches!(error, HubError::Core(Error::Client(_))), "{error}");
    drop((hub, source));
    fake.server.join().unwrap();
}

#[tokio::test]
async fn a_worktree_chat_starts_on_the_core_in_its_new_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let repository = committed_repository(dir.path());
    let root = dir.path().join("worktrees");
    let (source, fake) = fake_with_chat(&EndpointId::random());
    source.poll().unwrap();
    let mut hub = hub_on(&source, Layout::default());
    hub.worktrees = Some(root.clone());
    let created = hub
        .create_workspace(
            repository.to_str().unwrap(),
            Some("topic"),
            Agent::Claude,
            None,
        )
        .await
        .unwrap();
    // Listed apart until Sheprd keeps a workspace for the new checkout.
    let checkout = root.join("project").join("topic");
    assert_eq!(
        created.workspace,
        new_workspace_id(checkout.to_str().unwrap())
    );
    assert!(checkout.join(".git").exists());
    let params = &request(&fake)["params"];
    assert_eq!(params["cwd"], checkout.to_str().unwrap());
    assert_eq!(
        params["labels"][sheprd_session::NEW_WORKSPACE_LABEL],
        checkout.to_str().unwrap()
    );
    drop((hub, source));
    fake.server.join().unwrap();
}

#[tokio::test]
async fn a_worktree_whose_chat_fails_leaves_the_branch_free_to_retry() {
    let dir = tempfile::tempdir().unwrap();
    let repository = committed_repository(dir.path());
    let root = dir.path().join("worktrees");
    let (source, fake) = fake();
    source.poll().unwrap();
    let mut hub = hub_on(&source, Layout::default());
    hub.worktrees = Some(root.clone());
    // The fake core refuses a folder of this name.
    let error = hub
        .create_workspace(
            repository.to_str().unwrap(),
            Some("refused"),
            Agent::Claude,
            None,
        )
        .await
        .err()
        .unwrap();
    assert!(matches!(error, HubError::Core(Error::Client(_))), "{error}");
    assert!(!root.join("project").join("refused").exists());
    let branches = sheprd_client::new_command("git")
        .current_dir(&repository)
        .args(["branch", "--list", "refused"])
        .output()
        .unwrap();
    assert!(branches.stdout.is_empty());
    drop((hub, source));
    fake.server.join().unwrap();
}

#[test]
fn only_a_start_that_surely_ran_nothing_may_lose_its_checkout() {
    use super::super::create::ran_nothing;
    use sheprd_core_client::{Error as CoreError, Method};
    let client = |error: CoreError| HubError::Core(Error::Client(error));
    assert!(ran_nothing(&HubError::Core(Error::Disconnected)));
    assert!(ran_nothing(&client(CoreError::Refused {
        method: Method::TerminalRun,
        kind: None,
        message: "no".to_owned(),
    })));
    assert!(!ran_nothing(&client(CoreError::Lost)));
    assert!(!ran_nothing(&client(CoreError::Timeout(
        Method::TerminalRun
    ))));
}

#[tokio::test]
async fn a_worktree_git_refuses_starts_no_chat() {
    let dir = tempfile::tempdir().unwrap();
    let (source, fake) = fake();
    source.poll().unwrap();
    let mut hub = hub_on(&source, Layout::default());
    hub.worktrees = Some(dir.path().join("worktrees"));
    let error = hub
        .create_workspace(
            dir.path().to_str().unwrap(),
            Some("topic"),
            Agent::Claude,
            None,
        )
        .await
        .err()
        .unwrap();
    assert!(
        matches!(error, HubError::Core(Error::Worktree(_))),
        "{error}"
    );
    assert!(fake.inputs.try_recv().is_err());
    drop((hub, source));
    fake.server.join().unwrap();
}

#[tokio::test]
async fn without_a_core_new_work_stays_with_herdr_one() {
    let (source, fake) = fake();
    let hub = hub_on(&source, Layout::default());
    assert!(hub.new_core_tab("w1").await.unwrap().is_none());
    drop((hub, source));
    fake.server.join().unwrap();
}

#[tokio::test]
async fn a_new_tab_opens_a_shell_in_the_workspace_folder_and_answers_its_pane() {
    let records = vec![record("t1"), record("t9")];
    let (source, fake) = fake_with_doc(false, Value::Null, records);
    let mut layout = source.poll().unwrap();
    layout.workspaces.push(workspace("w1", "/code/one"));
    let hub = hub_on(&source, layout);
    let answer = hub.new_core_tab("w1").await.unwrap().unwrap();
    assert_eq!(answer["pane"]["pane_id"], id("t9"));
    let run = request(&fake);
    assert_eq!(run["params"]["cwd"], "/code/one");
    assert_eq!(run["params"]["keep"], true);
    assert!(run["params"].get("argv").is_none());
    assert_eq!(
        run["params"]["labels"][sheprd_session::WORKSPACE_LABEL],
        "w1"
    );
    drop((hub, source));
    fake.server.join().unwrap();
}

#[tokio::test]
async fn a_new_tab_in_a_named_session_names_the_session_apart() {
    let records = vec![record("t1"), record("t9")];
    let (source, fake) = fake_with_doc(false, Value::Null, records);
    let mut layout = source.poll().unwrap();
    layout
        .workspaces
        .push(workspace("herdr:dev:w1", "/code/one"));
    let hub = hub_on(&source, layout);
    hub.new_core_tab("herdr:dev:w1").await.unwrap().unwrap();
    let labels = &request(&fake)["params"]["labels"];
    assert_eq!(labels[sheprd_session::WORKSPACE_LABEL], "w1");
    assert_eq!(labels[sheprd_session::SESSION_LABEL], "dev");
    drop((hub, source));
    fake.server.join().unwrap();
}

#[tokio::test]
async fn a_second_tab_in_a_phones_new_workspace_joins_it_by_folder() {
    let records = vec![record("t1"), record("t9")];
    let (source, fake) = fake_with_doc(false, Value::Null, records);
    let mut layout = source.poll().unwrap();
    // Listed under its folder until Sheprd keeps a workspace for it.
    let new = new_workspace_id("/code/one");
    layout.workspaces.push(workspace(&new, "/code/one"));
    let hub = hub_on(&source, layout);
    hub.new_core_tab(&new).await.unwrap().unwrap();
    let labels = &request(&fake)["params"]["labels"];
    assert_eq!(labels[sheprd_session::NEW_WORKSPACE_LABEL], "/code/one");
    assert!(labels.get(sheprd_session::WORKSPACE_LABEL).is_none());
    drop((hub, source));
    fake.server.join().unwrap();
}

#[test]
fn work_in_a_phones_new_workspace_is_listed_in_one_row() {
    let labelled = |address: &str| {
        let mut record = record(address);
        record["labels"] = json!({sheprd_session::NEW_WORKSPACE_LABEL: "/code/one"});
        record
    };
    let records = vec![labelled("t1"), labelled("t9"), record("t2")];
    let (source, fake) = fake_with_doc(false, Value::Null, records);
    let layout = source.poll().unwrap();
    let rows: Vec<_> = layout
        .workspaces
        .iter()
        .filter(|row| row.cwd == "/code/one")
        .collect();
    assert_eq!(rows.len(), 1, "one workspace for the folder");
    assert_eq!(rows[0].id, new_workspace_id("/code/one"));
    assert_eq!(rows[0].terminals.len(), 2);
    drop(source);
    fake.server.join().unwrap();
}

#[test]
fn closing_ends_one_named_terminal_and_refuses_a_tab_it_does_not_list() {
    let (source, fake) = fake();
    source.poll().unwrap();
    source
        .call(ApiMethod::PaneClose, &json!({"pane_id": id("t1")}))
        .unwrap();
    let kill = request(&fake);
    assert_eq!(kill["method"], "terminal.kill");
    assert_eq!(kill["params"]["terminal"], "t1");
    let tab = json!({"tab_id": "core:app-tab:ab"});
    assert!(matches!(
        source.call(ApiMethod::TabClose, &tab).unwrap_err(),
        HubError::Core(Error::Unsupported)
    ));
    drop(source);
    fake.server.join().unwrap();
}

fn tab_of(root: Value) -> Value {
    json!({"spaces": [{"id": "sp", "name": "desk", "folder": "/project",
        "tabs": [{"id": "tab", "name": "", "root": root}]}]})
}

fn block(address: &str) -> Value {
    json!({"type": "block", "content": {"terminal": address}})
}

#[test]
fn closing_a_tab_of_one_pane_ends_that_terminal() {
    let (source, fake) = fake_with_doc(false, tab_of(block("t1")), vec![record("t1")]);
    let layout = source.poll().unwrap();
    let tab = layout.terminal(&id("t1")).unwrap().1.tab.clone();
    source
        .call(ApiMethod::TabClose, &json!({"tab_id": tab}))
        .unwrap();
    assert_eq!(request(&fake)["params"]["terminal"], "t1");
    drop(source);
    fake.server.join().unwrap();
}

#[test]
fn closing_a_tab_of_a_shell_and_a_chat_ends_both() {
    let endpoint = EndpointId::random();
    let root = json!({"type": "row", "children": [block("t1"), block("t9")]});
    let records = vec![record("t1"), chat_record("t9", &endpoint)];
    let (source, fake) = fake_with_doc(false, tab_of(root), records);
    let layout = source.poll().unwrap();
    let tab = layout.terminal(&id("t1")).unwrap().1.tab.clone();
    assert_eq!(layout.chat(&id("t9")).unwrap().1.tab, tab);
    source
        .call(ApiMethod::TabClose, &json!({"tab_id": tab}))
        .unwrap();
    let mut ended = [request(&fake), request(&fake)]
        .map(|kill| kill["params"]["terminal"].as_str().unwrap().to_owned());
    ended.sort();
    assert_eq!(ended, ["t1", "t9"]);
    assert!(lock(&source.state).terminals.is_empty());
    drop(source);
    fake.server.join().unwrap();
}

#[test]
fn a_saved_tab_names_its_id_and_the_panes_terminal() {
    let (source, fake) = fake();
    source.poll().unwrap();
    assert_eq!(
        source.saved_target("core:tab:4", &id("t1")),
        Some((4, "t1".to_owned()))
    );
    // A desktop space's tab, or a terminal no tab holds, is not Sheprd's.
    assert_eq!(source.saved_target("core:app-tab:ab", &id("t1")), None);
    assert_eq!(source.saved_target(&id("t1"), &id("t1")), None);
    assert_eq!(source.saved_target("core:tab:4", &id("t404")), None);
    drop(source);
    fake.server.join().unwrap();
}

#[tokio::test]
async fn a_named_herdr_1_session_is_refused_rather_than_ignored() {
    let (source, fake) = fake_with_doc(false, Value::Null, vec![record("t1")]);
    let layout = source.poll().unwrap();
    let mut hub = hub_on(&source, layout);
    hub.sessions = crate::hub::sessions::Sessions::offline_with(
        "dev",
        Layout::default(),
        PathBuf::from("/nonexistent/herdr-client.sock"),
    );
    let refused = hub
        .create_workspace("/code/one", None, Agent::Claude, Some("dev"))
        .await;
    assert!(
        matches!(refused, Err(HubError::SessionOnHerdr2)),
        "{refused:?}"
    );
    assert!(
        fake.inputs.try_recv().is_err(),
        "nothing started on Herdr 2"
    );
    drop((hub, source));
    fake.server.join().unwrap();
}
