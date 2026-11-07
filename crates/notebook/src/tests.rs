use crate::{CellId, CellKind, CellState, Kernel, Notebook, Problem, RunMode, RunRequest, Runner};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn runner() -> Runner {
    Runner::new(Arc::new(AtomicBool::new(false)))
}

fn build(sources: &[&str]) -> (Notebook, Vec<CellId>) {
    let mut nb = Notebook::new();
    let ids = sources.iter().map(|s| nb.add_cell(None, CellKind::Code, s)).collect();
    (nb, ids)
}

fn out(nb: &Notebook, id: CellId) -> String {
    match nb.cell(id).and_then(|c| c.output.as_ref()) {
        Some(o) => match &o.result {
            Ok(Some(v)) => v.to_string(),
            Ok(None) => o.stdout.trim_end().to_string(),
            Err(e) => format!("ERROR {e}"),
        },
        None => "(no output)".into(),
    }
}

fn state(nb: &Notebook, id: CellId) -> CellState {
    nb.cell(id).map(|c| c.state).unwrap_or(CellState::New)
}

#[test]
fn run_all_follows_dependencies_not_page_order() {
    // The cell that uses `b` is first on the page.
    let (mut nb, ids) = build(&["b + 1", "b = a * 3", "a = 2"]);
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    assert_eq!(out(&nb, ids[0]), "7");
    assert!(ids.iter().all(|i| state(&nb, *i) == CellState::Fresh));
}

#[test]
fn editing_a_cell_reruns_its_dependents() {
    let (mut nb, ids) = build(&["a = 2", "b = a * 3", "b + 1"]);
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    assert_eq!(out(&nb, ids[2]), "7");
    nb.set_source(ids[0], "a = 10");
    assert_eq!(state(&nb, ids[0]), CellState::Stale);
    nb.run_blocking(&mut r, RunRequest::Cell(ids[0]));
    assert_eq!(out(&nb, ids[2]), "31");
    assert!(ids.iter().all(|i| state(&nb, *i) == CellState::Fresh));
}

#[test]
fn lazy_mode_marks_dependents_stale_instead_of_running_them() {
    let (mut nb, ids) = build(&["a = 2", "b = a * 3"]);
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    nb.mode = RunMode::Lazy;
    nb.set_source(ids[0], "a = 5");
    nb.run_blocking(&mut r, RunRequest::Cell(ids[0]));
    assert_eq!(state(&nb, ids[1]), CellState::Stale);
    assert_eq!(out(&nb, ids[1]), ""); // assignments show no value; it still holds the old result
    nb.run_blocking(&mut r, RunRequest::Stale);
    assert_eq!(state(&nb, ids[1]), CellState::Fresh);
}

#[test]
fn running_a_cell_first_runs_upstream_cells_that_never_ran() {
    let (mut nb, ids) = build(&["a = 4", "a * a"]);
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::Cell(ids[1]));
    assert_eq!(out(&nb, ids[1]), "16");
}

#[test]
fn a_name_may_only_be_defined_once() {
    let (mut nb, ids) = build(&["x = 1", "x = 2", "y = x"]);
    let kinds: Vec<_> = nb.cell(ids[0]).unwrap().problems.iter().map(|p| p.code()).collect();
    assert_eq!(kinds, vec!["MC002"]);
    assert!(matches!(nb.cell(ids[2]).unwrap().problems[0], Problem::AmbiguousReference { .. }));
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    assert!(ids.iter().all(|i| state(&nb, *i) == CellState::Blocked));
    assert_eq!(nb.check().len(), 3);
}

#[test]
fn private_names_do_not_collide() {
    let (mut nb, ids) = build(&["_t = 1\na = _t + 1", "_t = 5\nb = _t + 1"]);
    assert!(nb.check().is_empty());
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    assert_eq!(out(&nb, ids[0]), "");
    assert_eq!(state(&nb, ids[1]), CellState::Fresh);
}

#[test]
fn cycles_are_rejected() {
    let (mut nb, ids) = build(&["a = b + 1", "b = a + 1"]);
    assert!(nb.cell(ids[0]).unwrap().problems.iter().any(|p| p.code() == "MC003"));
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    assert_eq!(state(&nb, ids[0]), CellState::Blocked);
}

#[test]
fn syntax_errors_block_only_their_cell() {
    let (mut nb, ids) = build(&["a = (1 +", "b = 2"]);
    assert_eq!(nb.cell(ids[0]).unwrap().problems[0].code(), "MC001");
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    assert_eq!(state(&nb, ids[0]), CellState::Blocked);
    assert_eq!(state(&nb, ids[1]), CellState::Fresh);
}

#[test]
fn errors_block_downstream_cells() {
    let (mut nb, ids) = build(&["a = 1/0", "b = a + 1", "c = 5"]);
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    assert_eq!(state(&nb, ids[0]), CellState::Error);
    assert_eq!(state(&nb, ids[1]), CellState::Blocked);
    assert_eq!(state(&nb, ids[2]), CellState::Fresh);
    // fixing the upstream cell and running it clears the chain
    nb.set_source(ids[0], "a = 1");
    nb.run_blocking(&mut r, RunRequest::Cell(ids[0]));
    assert_eq!(state(&nb, ids[1]), CellState::Fresh);
    assert_eq!(out(&nb, ids[0]), "");
}

#[test]
fn deleting_a_cell_removes_its_variables() {
    let (mut nb, ids) = build(&["k = 3", "k * 2"]);
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    assert_eq!(out(&nb, ids[1]), "6");
    nb.delete_cell(ids[0]);
    r.forget_cell(ids[0]);
    assert_eq!(state(&nb, ids[1]), CellState::Stale);
    nb.run_blocking(&mut r, RunRequest::Cell(ids[1]));
    // `k` is now just a symbol again
    assert_eq!(out(&nb, ids[1]), "2*k");
}

#[test]
fn functions_defined_in_one_cell_work_in_another() {
    let (mut nb, ids) = build(&["def area(r):\n    return pi * r**2", "area(3)", "scale = 2\narea(scale)"]);
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    assert_eq!(out(&nb, ids[1]), "9*pi");
    assert_eq!(out(&nb, ids[2]), "4*pi");
}

#[test]
fn function_bodies_create_dependencies_on_globals() {
    let (mut nb, ids) = build(&["g = 9", "def f(t):\n    return g * t", "f(2)"]);
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    assert_eq!(out(&nb, ids[2]), "18");
    nb.set_source(ids[0], "g = 10");
    nb.run_blocking(&mut r, RunRequest::Cell(ids[0]));
    assert_eq!(out(&nb, ids[2]), "20");
}

#[test]
fn text_round_trip_keeps_cells() {
    let (nb, _) = build(&["a = 1", "a + 1"]);
    let text = nb.to_text();
    let back = Notebook::from_text(&text);
    assert_eq!(back.cells().iter().map(|c| c.source.as_str()).collect::<Vec<_>>(), vec!["a = 1", "a + 1"]);
}

#[test]
fn kernel_thread_runs_jobs_and_can_be_interrupted() {
    let (mut nb, ids) = build(&["a = 6 * 7", "while True:\n    pass", "z = a"]);
    let kernel = Kernel::spawn(|| {}).unwrap();
    let jobs = nb.plan(RunRequest::Cell(ids[0]));
    kernel.run(jobs);
    let mut seen = 0;
    while seen < 2 {
        match kernel.events.recv_timeout(Duration::from_secs(10)).unwrap() {
            crate::Event::Started(id) => nb.mark_running(id),
            crate::Event::Finished(id, r) => {
                nb.apply(id, r);
                seen += 1;
            }
            crate::Event::Reset => nb.mark_all_stale(),
        }
    }
    assert_eq!(state(&nb, ids[0]), CellState::Fresh);
    // a runaway cell: interrupt it
    let jobs = nb.plan(RunRequest::Cell(ids[1]));
    kernel.run(jobs);
    loop {
        match kernel.events.recv_timeout(Duration::from_secs(10)).unwrap() {
            crate::Event::Started(_) => kernel.interrupt(),
            crate::Event::Finished(id, r) => {
                nb.apply(id, r);
                break;
            }
            crate::Event::Reset => {}
        }
    }
    assert_eq!(state(&nb, ids[1]), CellState::Error);
    assert!(out(&nb, ids[1]).contains("cancelled"), "{}", out(&nb, ids[1]));
}
