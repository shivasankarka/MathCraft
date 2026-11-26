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
    kernel.run(jobs, None);
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
    kernel.run(jobs, None);
    loop {
        match kernel.events.recv_timeout(Duration::from_secs(10)).unwrap() {
            crate::Event::Started(_) => {
                // let the loop really start: an interrupt that lands before the kernel's pre-run check cancels the
                // cell without running it (Stale), which is also correct but not what this test is about
                std::thread::sleep(Duration::from_millis(300));
                kernel.interrupt();
            }
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

#[test]
fn restore_brings_back_deleted_cells_and_keeps_typing() {
    let mut nb = Notebook::from_text("# %%\nx = 1\n# %%\ny = x\n");
    let ids: Vec<CellId> = nb.cells().iter().map(|c| c.id).collect();
    let before = nb.layout();
    nb.delete_cell(ids[1]);
    nb.set_source(ids[0], "x = 2");
    let removed = nb.restore(&before);
    assert!(removed.is_empty());
    assert_eq!(nb.cells().iter().map(|c| c.id).collect::<Vec<_>>(), ids);
    assert_eq!(nb.cells().first().map(|c| c.source.as_str()), Some("x = 2"));
    assert_eq!(nb.cells().get(1).map(|c| c.source.as_str()), Some("y = x"));
    let after = nb.layout();
    nb.add_cell(None, CellKind::Code, "z = 1");
    assert_eq!(nb.restore(&after).len(), 1);
}

fn ran(text: &str) -> Notebook {
    let mut nb = Notebook::from_text(text);
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::All);
    nb
}

const SAVED: &str = "# %%\na = x**2 + 1\n\n# %%\nb = diff(a, x)\nb\n\n# %%\nprint('hi')\n\n# %%\nplot(sin(x), (x, 0, 1))\n";

#[test]
fn saved_results_come_back_dimmed_and_only_for_cells_that_are_unchanged() {
    let json = ran(SAVED).export_outputs();
    let mut nb = Notebook::from_text(SAVED);
    assert_eq!(nb.import_outputs(&json), 3, "the plot is not saved");
    let states: Vec<_> = nb.cells().iter().map(|c| c.state).collect();
    assert_eq!(states, [CellState::Stale, CellState::Stale, CellState::Stale, CellState::New]);
    let shown = nb.cells().get(1).and_then(|c| c.output.as_ref()).map(|o| format!("{:?}", o.result));
    assert!(shown.as_ref().is_some_and(|s| s.contains("2*x")), "{shown:?}");
    assert_eq!(nb.cells().get(2).and_then(|c| c.output.as_ref()).map(|o| o.stdout.as_str()), Some("hi\n"));

    // changing a cell drops its result and those of the cells that read from it
    let mut edited = Notebook::from_text(&SAVED.replace("x**2 + 1", "x**3"));
    assert_eq!(edited.import_outputs(&json), 1, "only the print cell is independent of it");
    assert_eq!(edited.cells().first().map(|c| c.state), Some(CellState::New));
    assert_eq!(edited.import_outputs("not json"), 0);
}

#[test]
fn opening_runs_only_cells_without_a_saved_result() {
    let json = ran(SAVED).export_outputs();
    let mut nb = Notebook::from_text(SAVED);
    nb.import_outputs(&json);
    let mut r = runner();
    nb.run_blocking(&mut r, RunRequest::New);
    assert_eq!(nb.cells().last().map(|c| c.state), Some(CellState::Fresh));
    assert_eq!(nb.cells().first().map(|c| c.state), Some(CellState::Stale), "saved results stay dimmed until they run");
    assert_eq!(nb.cells().get(2).map(|c| c.state), Some(CellState::Stale));
    // running a saved cell's dependent brings its stale inputs back first
    let id = nb.cells().get(1).map(|c| c.id).unwrap_or_default();
    nb.run_blocking(&mut r, RunRequest::Cell(id));
    assert_eq!(nb.cells().get(1).map(|c| c.state), Some(CellState::Fresh));
}

#[test]
fn errors_are_saved_and_values_that_do_not_read_back_are_not() {
    let src = "# %%\n1/0\n\n# %%\ns = 'it\\'s'\ns\n";
    let json = ran(src).export_outputs();
    let mut nb = Notebook::from_text(src);
    nb.import_outputs(&json);
    let first = nb.cells().first().and_then(|c| c.output.as_ref());
    assert!(first.is_some_and(|o| o.result.is_err()));
    assert_eq!(nb.cells().get(1).map(|c| c.state), Some(CellState::New), "strings are left to run again");
}
