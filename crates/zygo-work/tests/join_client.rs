//! CC-4: the work join keys a request by its agent id unless its client sends no per-agent id (K§6.1.2 concern 9).

use zygo_usage::{RequestRow, WorkUnitRow};
use zygo_work::join;

fn wu(unit: &str, session: &str, agent: &str, began: i64) -> WorkUnitRow {
    WorkUnitRow { unit: unit.into(), round: 1, session: session.into(), agent_id: agent.into(), began_at: began, ..Default::default() }
}
fn rq(session: &str, agent: &str, agent_id: Option<&str>, ts: i64) -> RequestRow {
    RequestRow { ts, session: session.into(), agent: agent.into(), agent_id: agent_id.map(String::from), model: "m1".into(),
                 role_served: Some("lead".into()), ..Default::default() }
}

#[test]
fn join_keys_an_unknown_client_by_agent_id() {
    let units = [wu("u1", "S1", "main", 100), wu("u2", "S1", "x", 200)];
    let reqs = [rq("S1", "codex", None, 150), rq("S1", "codex", None, 250)];
    let got: Vec<(String, Vec<i64>)> =
        join::join(&units, &reqs).iter().map(|x| (x.row.unit.clone(), x.requests.iter().map(|r| r.ts).collect())).collect();
    assert_eq!(got, vec![("u1".to_string(), vec![150, 250]), ("u2".to_string(), vec![])]);
}
