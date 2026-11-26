use super::*;
use egui_kittest::kittest::Queryable;
use std::time::Instant;

#[test]
fn results_can_be_copied_in_several_formats() {
    use mathcraft_core::Expr;
    let e = Value::from_expr(Expr::pow(&Expr::sym("x"), &Expr::int(2)));
    let formats: Vec<_> = copy_formats(&e).into_iter().collect();
    let get = |name: &str| formats.iter().find(|(n, _)| *n == name).and_then(|(_, t)| t.clone());
    assert_eq!(get("Text").as_deref(), Some("x**2"));
    assert_eq!(get("LaTeX").as_deref(), Some("x^{2}"));
    assert_eq!(get("Python").as_deref(), Some("x**2"));
    assert_eq!(get("Rust").as_deref(), Some("x.powi(2)"));
    // a string has no LaTeX or code form
    assert!(copy_formats(&Value::Str("hi".into())).iter().filter(|(n, _)| *n != "Text").all(|(_, t)| t.is_none()));
}

#[test]
fn settings_are_saved_and_restored() {
    let dir = std::env::temp_dir().join(format!("mathcraft-app-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.load_settings(Some(dir.clone()));
    app.run_command(Cmd::ToggleVim, &ctx);
    app.run_command(Cmd::Theme(ThemePreference::Dark), &ctx);
    app.run_command(Cmd::Accent(Some("purple")), &ctx);
    frame(&ctx, &mut app);
    let saved = settings::Settings::load(&dir);
    assert!(saved.vim);
    assert_eq!(saved.appearance, ThemePreference::Dark);
    assert_eq!(saved.accent.as_deref(), Some("purple"));
    // a fresh app picks them up
    let ctx2 = egui::Context::default();
    let mut again = App::with_context(ctx2.clone(), None);
    again.load_settings(Some(dir.clone()));
    assert!(again.vim);
    assert_eq!(ctx2.options(|o| o.theme_preference), ThemePreference::Dark);
    assert_eq!(again.dark.accent, egui::Color32::from_rgb(168, 128, 255));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_session_comes_back_with_tabs_folder_and_recent_files() {
    let base = std::env::temp_dir().join(format!("mathcraft-app-session-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (cfg, work) = (base.join("cfg"), base.join("work"));
    std::fs::create_dir_all(&work).unwrap();
    for f in ["a.py", "b.py", "c.py"] {
        std::fs::write(work.join(f), "# %%\nx = 1\n").unwrap();
    }
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.load_settings(Some(cfg.clone()));
    app.set_folder(work.clone());
    app.open_path(work.join("a.py"));
    app.open_path(work.join("b.py"));
    app.open_path(work.join("c.py"));
    app.open_path(work.join("b.py"));
    frame(&ctx, &mut app);
    let saved = session::Session::load(&cfg);
    assert_eq!(saved.tabs.len(), 3);
    assert_eq!(saved.folder.as_deref(), Some(work.as_path()));
    assert_eq!(saved.recent.first(), Some(&work.join("c.py")));
    // a file deleted since is skipped; the active tab (b) is selected again
    std::fs::remove_file(work.join("a.py")).unwrap();
    let ctx2 = egui::Context::default();
    let mut again = App::with_context(ctx2.clone(), None);
    again.load_settings(Some(cfg.clone()));
    again.load_session(false);
    assert_eq!(again.order.len(), 2);
    assert_eq!(again.cur.path.as_deref(), Some(work.join("b.py").as_path()));
    assert_eq!(again.folder.as_deref(), Some(work.as_path()));
    assert!(again.recent.contains(&work.join("a.py")));
    // a file named on the command line wins: no tabs are restored
    let mut named = App::with_context(egui::Context::default(), None);
    named.load_settings(Some(cfg.clone()));
    named.load_session(true);
    assert_eq!(named.order.len(), 1);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn coordinates_use_the_precision_the_view_needs() {
    let b = |w: f64| egui_plot::PlotBounds::from_min_max([0.0, 0.0], [w, w]);
    assert_eq!(coordinate_text(1.23456, -0.5, &b(10.0)), "x = 1.23   y = -0.5");
    assert_eq!(coordinate_text(1.23456, 2.0, &b(0.01)), "x = 1.23456   y = 2");
    assert_eq!(coordinate_text(1234.4, 7.0, &b(5000.0)), "x = 1234   y = 7");
}

#[test]
fn a_plot_cell_draws_and_resets() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.replace_notebook(Notebook::from_text("# %%\nplot(sin(x), (x, 0, 6))\n"), None);
    let start = Instant::now();
    while app.busy() || start.elapsed() < Duration::from_millis(200) {
        frame(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(app.cur.nb.cells().first().and_then(|c| c.output.as_ref()).map(|o| &o.result), Some(Ok(Some(Value::Plot(_))))));
    let id = app.cur.nb.cells().first().map(|c| c.id).unwrap_or(0);
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("plot", id)).with("reset"), ResetPlot(true)));
    frame(&ctx, &mut app);
    assert!(ctx.data(|d| d.get_temp::<ResetPlot>(egui::Id::unique(("plot", id)).with("reset"))).is_some_and(|r| !r.0), "the request is consumed");
}

#[test]
fn plot_options_are_applied_and_marks_cycle() {
    assert_eq!(Marks::Auto.next().next().next(), Marks::Auto);
    assert!(Marks::Wide.spacing().start() > Marks::Auto.spacing().start());
    assert!(Marks::Tight.spacing().start() < Marks::Auto.spacing().start());
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.replace_notebook(Notebook::from_text("# %%\nplot(sin(x), (x, 0, 6))\n"), None);
    let start = Instant::now();
    while app.busy() || start.elapsed() < Duration::from_millis(200) {
        frame(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(10));
    }
    let id = app.cur.nb.cells().first().map(|c| c.id).unwrap_or(0);
    let key = egui::Id::unique(("plot", id)).with("options");
    let chosen = PlotOptions { grid: true, axes: false, marks: Marks::Tight };
    ctx.data_mut(|d| d.insert_temp(key, chosen));
    frame(&ctx, &mut app);
    assert_eq!(ctx.data(|d| d.get_temp::<PlotOptions>(key)), Some(chosen), "drawing with the options keeps them");
}

#[test]
fn tick_labels_are_short() {
    assert_eq!(tick_label(0.0), "0");
    assert_eq!(tick_label(-0.0), "0");
    assert_eq!(tick_label(0.2), "0.2");
    assert_eq!(tick_label(2.5), "2.5");
    assert_eq!(tick_label(10.0), "10");
    assert_eq!(tick_label(-0.30000000000000004), "-0.3");
}

fn frame(ctx: &egui::Context, app: &mut App) {
    ctx.run_ui(egui::RawInput::default(), |ui| app.show(ui)).drop_without_applying_deltas();
}

/// Render real frames of the welcome notebook until the kernel has finished every cell.
#[test]
fn welcome_notebook_renders_and_runs() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    let start = Instant::now();
    while app.busy() || start.elapsed() < Duration::from_millis(300) {
        frame(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(20));
        assert!(start.elapsed() < Duration::from_secs(60), "the welcome notebook did not finish running");
    }
    frame(&ctx, &mut app);
    let code: Vec<_> = app.cur.nb.cells().iter().filter(|c| c.kind == CellKind::Code).collect();
    assert!(code.len() >= 8);
    for c in &code {
        assert_eq!(c.state, CellState::Fresh, "cell `{}` is {:?}: {:?}", c.source, c.state, c.output);
    }
    assert!(app.cur.nb.check().is_empty());
    let show = |i: usize| match app.cur.nb.cells().get(i).and_then(|c| c.output.as_ref()).map(|o| &o.result) {
        Some(Ok(Some(v))) => v.to_string().chars().take(200).collect::<String>(),
        other => format!("{other:?}").chars().take(200).collect(),
    };
    assert!(show(2).contains("cos(x)"), "{}", show(2));
    assert!(show(3).starts_with("<plot"), "{}", show(3));
}

/// Every bundled example opens in its own tab, runs to the end without an error, and is listed in the palette.
#[test]
fn bundled_examples_open_and_run() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    for (i, (name, ..)) in examples::EXAMPLES.iter().enumerate() {
        app.open_example(i);
        let start = Instant::now();
        while app.busy() || start.elapsed() < Duration::from_millis(200) {
            frame(&ctx, &mut app);
            std::thread::sleep(Duration::from_millis(20));
            assert!(start.elapsed() < Duration::from_secs(60), "the example {name} did not finish running");
        }
        for c in app.cur.nb.cells().iter().filter(|c| c.kind == CellKind::Code) {
            let failed = matches!(c.output.as_ref().map(|o| &o.result), Some(Err(_)));
            assert!(!failed && c.state == CellState::Fresh, "{name}: cell `{}` is {:?}: {:?}", c.source, c.state, c.output);
        }
    }
    let labels: Vec<String> = app.commands(&ctx).into_iter().filter(|i| i.group == "Example").map(|i| i.title).collect();
    assert_eq!(labels.len(), examples::EXAMPLES.len());
}

/// Text cells with a table, links, struck text and inline math draw without trouble.
#[test]
fn markdown_tables_and_links_render() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    let text =
        "# Notes\n\n| f | F |\n|---|---|\n| $x^2$ | **big** |\n| plain | [site](https://example.org) |\n\nSee ~~old~~ [docs](https://example.org) and $y$.";
    app.cur.nb.add_cell(None, CellKind::Markdown, text);
    for _ in 0..3 {
        frame(&ctx, &mut app);
    }
    assert!(app.cur.nb.cells().iter().any(|c| c.source == text));
}

/// Cmd+F opens the window, which lists matches; the palette jumps to where a name is defined.
#[test]
fn find_window_and_go_to_definition() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.replace_notebook(Notebook::from_text("# %%\nalpha = 1\n\n# %%\nbeta = alpha + 1\n"), None);
    app.open_find();
    if let Some(f) = app.find.as_mut() {
        f.query.find = "alpha".into();
    }
    for _ in 0..3 {
        frame(&ctx, &mut app);
    }
    assert!(app.find.is_some(), "the window stays open");
    let hits = find::hits(&app.cur.nb, &app.find.as_ref().map(|f| f.query.clone()).unwrap_or_default());
    assert_eq!(hits.len(), 2);

    let defs: Vec<_> = app.commands(&ctx).into_iter().filter(|i| i.group == "Definition").map(|i| (i.title, i.detail)).collect();
    assert_eq!(defs, vec![("alpha".to_owned(), "cell 1".to_owned()), ("beta".to_owned(), "cell 2".to_owned())]);
    let target = app.cur.nb.cells().get(1).map(|c| c.id);
    app.run_command(Cmd::GoTo(target.unwrap_or_default()), &ctx);
    assert_eq!(app.cur.focus, target);
}

#[test]
fn editing_and_running_through_actions_updates_dependents() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.replace_notebook(Notebook::from_text("# %%\na = 2\n# %%\nb = a * 10\n# %%\nb + 1\n"), None);
    let wait = |ctx: &egui::Context, app: &mut App| {
        let start = Instant::now();
        while app.busy() || start.elapsed() < Duration::from_millis(200) {
            frame(ctx, app);
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    wait(&ctx, &mut app);
    let ids: Vec<CellId> = app.cur.nb.cells().iter().map(|c| c.id).collect();
    let last = |app: &App| app.cur.nb.cells().last().and_then(|c| c.output.as_ref()).map(|o| format!("{:?}", o.result)).unwrap_or_default();
    assert!(last(&app).contains("21"), "{}", last(&app));
    app.apply(vec![Action::SetSource(ids[0], "a = 5".into()), Action::Run(ids[0])]);
    wait(&ctx, &mut app);
    assert!(last(&app).contains("51"), "{}", last(&app));
    assert!(app.cur.dirty);
}

#[test]
fn moving_a_slider_reruns_its_dependents() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.replace_notebook(Notebook::from_text("# %%\na = slider(0, 10, 2)\n# %%\nb = a * 10\n# %%\nb + 1\n"), None);
    let wait = |ctx: &egui::Context, app: &mut App| {
        let start = Instant::now();
        while app.busy() || start.elapsed() < Duration::from_millis(200) {
            frame(ctx, app);
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    wait(&ctx, &mut app);
    let last = |app: &App| app.cur.nb.cells().last().and_then(|c| c.output.as_ref()).map(|o| format!("{:?}", o.result)).unwrap_or_default();
    assert!(last(&app).contains("21"), "{}", last(&app));
    let first = app.cur.nb.cells().first().map(|c| (c.id, c.source.clone())).unwrap();
    let spec = slider::parse(&first.1).expect("a slider cell");
    app.apply(vec![Action::SetSource(first.0, spec.with_value(7.2)), Action::Run(first.0)]);
    wait(&ctx, &mut app);
    assert_eq!(app.cur.nb.cells().first().map(|c| c.source.as_str()), Some("a = slider(0, 10, 7)"));
    assert!(last(&app).contains("71"), "{}", last(&app));
}

#[test]
fn tabs_can_be_reordered_and_the_order_is_remembered() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.open_in_tab(Notebook::from_text("# %%\na = 1\n"), Some(PathBuf::from("/tmp/mc_r1.py")));
    app.open_in_tab(Notebook::from_text("# %%\nb = 2\n"), Some(PathBuf::from("/tmp/mc_r2.py")));
    let before = app.order.clone();
    app.move_tab(before[2], 0);
    assert_eq!(app.order, vec![before[2], before[0], before[1]]);
    app.move_tab(before[2], 99);
    assert_eq!(app.order, vec![before[0], before[1], before[2]]);
    assert_eq!(app.order.len(), 3);
    frame(&ctx, &mut app);
}

#[test]
fn tabs_open_switch_and_close() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    let first = app.cur.id;
    app.open_in_tab(Notebook::from_text("# %%\na = 1\n"), Some(PathBuf::from("/tmp/mc_a.py")));
    assert_eq!(app.order.len(), 2, "the welcome notebook is not pristine, so a tab is added");
    let second = app.cur.id;
    app.open_in_tab(Notebook::from_text("# %%\nb = 2\n"), Some(PathBuf::from("/tmp/mc_b.py")));
    assert_eq!(app.order.len(), 3);
    app.select_tab(first);
    assert_eq!(app.cur.id, first);
    assert!(app.cur.nb.cells().len() > 3, "the welcome notebook came back intact");
    app.step_tab(1);
    assert_eq!(app.cur.id, second);
    app.step_tab(-1);
    app.step_tab(-1);
    assert_ne!(app.cur.id, first, "wraps around");
    frame(&ctx, &mut app);
    app.close_tab(second);
    assert_eq!(app.order.len(), 2);
    assert!(app.doc(second).is_none());
    let remaining: Vec<u64> = app.order.clone();
    for id in remaining {
        app.close_tab(id);
    }
    assert_eq!(app.order.len(), 1, "closing the last tab leaves a blank one");
    assert!(app.cur.is_pristine());
    frame(&ctx, &mut app);
}

fn sample_folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mathcraft-app-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lectures")).unwrap();
    std::fs::write(dir.join("derivatives.py"), "# %%\nf = sin(x)**2\n# %%\ndiff(f, x)\n").unwrap();
    std::fs::write(dir.join("solving.py"), "# %%\nsolve(x**3 - 3*x + 1, x)\n").unwrap();
    std::fs::write(dir.join("lectures/week1.py"), "# %%\n1/3 + 1/6\n").unwrap();
    dir
}

/// Make `dir` a repository with one commit, then change one notebook and add another. `false` without `git`.
fn init_git(dir: &Path) -> bool {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let ok = run(&["init", "-q", "-b", "main"]) && run(&["add", "."]) && run(&["commit", "-q", "-m", "first"]);
    std::fs::write(dir.join("derivatives.py"), "# %%\nf = sin(x)**3\n# %%\ndiff(f, x)\n").unwrap();
    std::fs::write(dir.join("lectures/week2.py"), "# %%\n2 + 2\n").unwrap();
    ok
}

#[test]
fn a_git_folder_shows_marks_and_the_branch() {
    let dir = sample_folder("git");
    if !init_git(&dir) {
        return;
    }
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), Some(dir.clone()));
    let start = Instant::now();
    while app.git.is_none() && start.elapsed() < Duration::from_secs(20) {
        frame(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(20));
    }
    let git = app.git.clone().unwrap_or_default();
    assert_eq!(git.summary(), "main · 2 changed");
    assert_eq!(git.mark(Path::new("derivatives.py"), false), Some(git::Mark::Modified));
    assert_eq!(git.mark(Path::new("lectures/week2.py"), false), Some(git::Mark::Untracked));
    assert_eq!(git.mark(Path::new("lectures"), true), Some(git::Mark::Untracked));
    assert_eq!(git.mark(Path::new("solving.py"), false), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn changes_compare_the_notebook_with_the_last_commit() {
    let dir = sample_folder("changes");
    if !init_git(&dir) {
        return;
    }
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), Some(dir.clone()));
    app.open_path(dir.join("derivatives.py"));
    app.open_changes();
    frame(&ctx, &mut app);
    let changes = app.changes.as_ref().unwrap();
    assert_eq!(diff::counts(&changes.lines), (1, 1), "sin(x)**2 became sin(x)**3: {:?}", changes.lines);
    assert!(changes.lines.iter().any(|l| l.kind == diff::Kind::Added && l.text == "f = sin(x)**3"));
    // an edit shows up without asking again
    let id = app.cur.nb.cells().last().map(|c| c.id).unwrap();
    app.apply(vec![Action::SetSource(id, "diff(f, x) + 1".into())]);
    frame(&ctx, &mut app);
    assert_eq!(diff::counts(&app.changes.as_ref().unwrap().lines), (2, 2));
    // a notebook that was never committed has nothing to compare with
    app.open_path(dir.join("lectures/week2.py"));
    app.open_changes();
    frame(&ctx, &mut app);
    assert!(app.changes.as_ref().unwrap().head.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_fills_the_sidebar_and_files_open_in_tabs() {
    let dir = sample_folder("open");
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), Some(dir.clone()));
    assert!(app.show_sidebar);
    let names: Vec<&str> = app.tree.iter().map(workspace::Node::name).collect();
    assert_eq!(names, vec!["lectures", "derivatives.py", "solving.py"]);
    frame(&ctx, &mut app);
    app.open_path(dir.join("solving.py"));
    app.open_path(dir.join("derivatives.py"));
    app.open_path(dir.join("solving.py"));
    assert_eq!(app.cur.name(), "solving.py");
    assert_eq!(app.order.len(), 2, "the blank tab was reused and the repeat open only switched");
    app.new_in_folder();
    assert_eq!(app.cur.name(), "untitled.py");
    assert!(app.tree.iter().any(|n| n.name() == "untitled.py"), "the list shows the new file");
    frame(&ctx, &mut app);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn rich_cells_screenshot() {
    let Some(out) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    let src = "# %% [markdown]\n# # Math in text\n# The roots of $x**2 - 2$ are $sqrt(2)$ and $-sqrt(2)$, and it costs $5 or $6.\n# $$ integrate(1/(x**2+1), x) $$\n# - a list item with $a/b + c$ inside\n# > a quote about $exp(I*pi) + 1$\n# %%\na = slider(0, 10, 3)\n# %%\nplot([sin(x), cos(x)], (x, 0, 6))\n";
    for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
        let mut harness = egui_kittest::Harness::builder().wgpu().with_size(vec2(900.0, 780.0)).build_ui_state(
            move |ui, app: &mut Option<App>| {
                app.get_or_insert_with(|| {
                    let mut app = App::with_context(ui.ctx().clone(), None);
                    app.replace_notebook(Notebook::from_text(src), None);
                    app
                })
                .show(ui)
            },
            None,
        );
        harness.ctx.set_theme(pref);
        let start = Instant::now();
        while harness.state().as_ref().is_none_or(App::busy) && start.elapsed() < Duration::from_secs(30) {
            harness.run_steps(3);
            std::thread::sleep(Duration::from_millis(30));
        }
        harness.run_steps(6);
        harness.render().unwrap().save(out.join(format!("rich-{name}.png"))).unwrap();
    }
}

#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn sidebar_screenshot() {
    let Some(out) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    let dir = sample_folder("shot");
    init_git(&dir);
    for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
        let d = dir.clone();
        let mut harness = egui_kittest::Harness::builder().wgpu().with_size(vec2(1000.0, 520.0)).build_ui_state(
            move |ui, app: &mut Option<App>| {
                app.get_or_insert_with(|| {
                    let mut app = App::with_context(ui.ctx().clone(), Some(d.clone()));
                    app.open_path(d.join("derivatives.py"));
                    app.open_path(d.join("solving.py"));
                    app
                })
                .show(ui)
            },
            None,
        );
        harness.ctx.set_theme(pref);
        let start = Instant::now();
        while harness.state().as_ref().is_none_or(App::busy) && start.elapsed() < Duration::from_secs(30) {
            harness.run_steps(3);
            std::thread::sleep(Duration::from_millis(30));
        }
        harness.run_steps(6);
        harness.render().unwrap().save(out.join(format!("sidebar-{name}.png"))).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn narrow_views_screenshot() {
    let Some(out) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    let dir = sample_folder("narrow");
    for (name, pref, text) in [("text-dark", ThemePreference::Dark, true), ("sidebar-light", ThemePreference::Light, false)] {
        let d = dir.clone();
        let mut harness = egui_kittest::Harness::builder().wgpu().with_size(vec2(560.0, 560.0)).build_ui_state(
            move |ui, app: &mut Option<App>| {
                app.get_or_insert_with(|| {
                    let mut app = App::with_context(ui.ctx().clone(), Some(d.clone()));
                    app.open_path(d.join("derivatives.py"));
                    app.open_path(d.join("solving.py"));
                    if text {
                        app.cur.set_view(View::Text);
                    }
                    app
                })
                .show(ui)
            },
            None,
        );
        harness.ctx.set_theme(pref);
        let start = Instant::now();
        while harness.state().as_ref().is_none_or(App::busy) && start.elapsed() < Duration::from_secs(30) {
            harness.run_steps(3);
            std::thread::sleep(Duration::from_millis(30));
        }
        harness.run_steps(6);
        harness.render().unwrap().save(out.join(format!("narrow-{name}.png"))).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn text_view_keeps_the_notebook_in_step() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.replace_notebook(Notebook::from_text("# %%\na = 1\n# %%\nb = a + 1\n"), None);
    let ids: Vec<CellId> = app.cur.nb.cells().iter().map(|c| c.id).collect();
    app.cur.set_view(View::Text);
    assert!(app.cur.text.contains("b = a + 1"));
    frame(&ctx, &mut app);
    // edit the first cell
    app.cur.text = app.cur.text.replace("a = 1", "a = 5");
    app.cur.sync_from_text();
    let now: Vec<CellId> = app.cur.nb.cells().iter().map(|c| c.id).collect();
    assert_eq!(now, ids, "editing keeps every cell's identity");
    assert_eq!(app.cur.nb.cells().first().map(|c| c.source.trim()), Some("a = 5"));
    // insert a cell in the middle: the last cell keeps its identity
    app.cur.text = app.cur.text.replace("# %%\nb = a + 1", "# %%\nc = 3\n# %%\nb = a + 1");
    app.cur.sync_from_text();
    let now: Vec<CellId> = app.cur.nb.cells().iter().map(|c| c.id).collect();
    assert_eq!(now.len(), 3);
    assert_eq!((now.first(), now.last()), (ids.first(), ids.last()));
    // the cursor decides which cell Shift+Enter runs
    let at = app.cur.text.find("c = 3").unwrap_or(0);
    assert_eq!(app.cur.cell_at(at), now.get(1).copied());
    // deleting text down to one cell
    app.cur.text = "# %%\nz = 1\n".into();
    app.cur.sync_from_text();
    assert_eq!(app.cur.nb.cells().len(), 1);
    frame(&ctx, &mut app);
    app.cur.set_view(View::Notebook);
    frame(&ctx, &mut app);
}

#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn text_view_screenshot() {
    let Some(out) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
        let mut harness = egui_kittest::Harness::builder().wgpu().with_size(vec2(1100.0, 600.0)).build_ui_state(
            |ui, app: &mut Option<App>| {
                app.get_or_insert_with(|| {
                    let mut app = App::with_context(ui.ctx().clone(), None);
                    app.replace_notebook(
                        Notebook::from_text("# %% [markdown]\n# # Derivatives\n# %%\nf = sin(x)**2\n# %%\ndiff(f, x)\n# %%\nsolve(x**3 - 3*x + 1, x)\n# %%\nf = 1/0\n# %%\nplot(sin(x)**2, (x, 0, 6))\n"),
                        None,
                    );
                    app.cur.set_view(View::Text);
                    // the Vim mode segment shows in the status bar
                    app.vim = true;
                    app
                })
                .show(ui)
            },
            None,
        );
        harness.ctx.set_theme(pref);
        let start = Instant::now();
        while harness.state().as_ref().is_none_or(App::busy) && start.elapsed() < Duration::from_secs(30) {
            harness.run_steps(3);
            std::thread::sleep(Duration::from_millis(30));
        }
        // put the cursor in the `solve` cell so the results panel marks it
        if let Some(app) = harness.state() {
            let id = text_view::editor_id(app.cur.id);
            let at = app.cur.text.find("solve").unwrap_or(0) + 2;
            let mut st = egui::TextEdit::load_state(&harness.ctx, id).unwrap_or_default();
            st.cursor.set_char_range(Some(CCursorRange::one(egui::text::CCursor::new(at))));
            st.store(&harness.ctx, id);
        }
        harness.run_steps(6);
        harness.render().unwrap().save(out.join(format!("text-{name}.png"))).unwrap();
    }
}

#[test]
fn an_untouched_blank_tab_is_reused_when_opening() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.replace_notebook(Notebook::from_text("# %%\n"), None);
    app.open_in_tab(Notebook::from_text("# %%\nx = 1\n"), Some(PathBuf::from("/tmp/mc_c.py")));
    assert_eq!(app.order.len(), 1);
    assert_eq!(app.cur.name(), "mc_c.py");
}

#[test]
fn delete_last_cell_leaves_an_empty_one() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.replace_notebook(Notebook::from_text("# %%\nx = 1\n"), None);
    let id = app.cur.nb.cells().first().map(|c| c.id).unwrap_or(0);
    app.apply(vec![Action::Delete(id)]);
    assert_eq!(app.cur.nb.cells().len(), 1);
    frame(&ctx, &mut app);
}

#[test]
fn cell_changes_undo_and_redo_without_losing_typing() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.replace_notebook(Notebook::from_text("# %%\nx = 1\n# %%\ny = x + 1\n# %%\nz = 3\n"), None);
    let ids: Vec<_> = app.cur.nb.cells().iter().map(|c| c.id).collect();
    app.apply(vec![Action::Delete(ids[1])]);
    assert_eq!(app.cur.nb.cells().len(), 2);
    // typing in a remaining cell after the delete must survive the undo
    app.apply(vec![Action::SetSource(ids[2], "z = 33".into())]);
    app.undo_cells();
    let cells = app.cur.nb.cells();
    assert_eq!(cells.iter().map(|c| c.id).collect::<Vec<_>>(), ids);
    assert_eq!(cells[1].source, "y = x + 1");
    assert_eq!(cells[2].source, "z = 33");
    app.redo_cells();
    assert_eq!(app.cur.nb.cells().len(), 2);
    app.undo_cells();
    app.apply(vec![Action::Move(ids[0], 1)]);
    assert_eq!(app.cur.nb.cells().first().map(|c| c.id), Some(ids[1]));
    app.undo_cells();
    assert_eq!(app.cur.nb.cells().first().map(|c| c.id), Some(ids[0]));
    // a new change drops the redo history
    app.apply(vec![Action::Delete(ids[0])]);
    app.undo_cells();
    assert!(app.cur.can_redo());
    app.apply(vec![Action::AddAfter(None, CellKind::Code)]);
    assert!(!app.cur.can_redo());
    frame(&ctx, &mut app);
}

/// Renders the notebook with the GPU, in both themes, to PNG files for a visual check. Skipped unless
/// Step the harness until no cell is running (at most about ten seconds).
fn settle(harness: &mut egui_kittest::Harness<'_, Option<App>>) {
    for _ in 0..500 {
        harness.run_steps(2);
        if !harness.state().as_ref().is_some_and(App::busy) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A headless harness around one empty code cell, with the editor focused.
fn typing_harness() -> egui_kittest::Harness<'static, Option<App>> {
    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(900.0, 600.0))
        .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
    // the welcome notebook is still running (a spinner repaints), so step until the kernel is idle instead of
    // waiting for the UI to settle; a slow machine needs more than a few frames
    settle(&mut harness);
    if let Some(app) = harness.state_mut() {
        app.replace_notebook(Notebook::from_text("# %%\n"), None);
    }
    settle(&mut harness);
    harness.run();
    harness.get_by_role(egui::accesskit::Role::MultilineTextInput).focus();
    harness.run();
    harness
}

fn code_cell(harness: &egui_kittest::Harness<'_, Option<App>>) -> Option<Cell> {
    harness.state().as_ref()?.cur.nb.cells().iter().find(|c| c.kind == CellKind::Code).cloned()
}

fn source(harness: &egui_kittest::Harness<'_, Option<App>>) -> String {
    code_cell(harness).map(|c| c.source).unwrap_or_default()
}

#[test]
fn tab_accepts_a_completion() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("y = dif");
    h.run();
    assert_eq!(source(&h), "y = dif");
    h.key_press(Key::Tab);
    h.run();
    assert_eq!(source(&h), "y = diff(");
}

#[test]
fn arrows_choose_and_escape_closes_the_list() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("fact");
    h.run();
    let first = complete::candidates("fact", &[]);
    assert!(first.len() >= 2, "the test needs two `fact` completions: {first:?}");
    h.key_press(Key::ArrowDown);
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let want = first.get(1).map(|c| format!("{}(", c.name)).unwrap_or_default();
    assert_eq!(source(&h), want);

    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("fact");
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Tab);
    h.run();
    assert_eq!(source(&h), "fact    ", "after Escape, Tab indents instead of completing");
}

#[test]
fn enter_keeps_indentation_and_shift_enter_runs() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("if True:");
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(source(&h), "if True:\n    ");
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("1 + 1");
    h.run();
    h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    let start = Instant::now();
    while code_cell(&h).map(|c| c.state) != Some(CellState::Fresh) && start.elapsed() < Duration::from_secs(20) {
        h.run_steps(2);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(code_cell(&h).map(|c| c.state), Some(CellState::Fresh));
}

#[test]
fn the_palette_runs_a_command() {
    let mut harness = typing_harness();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::P);
    harness.run();
    assert!(harness.state().as_ref().is_some_and(|a| a.palette.open), "Cmd+P opens the palette");
    harness.get_by_role(egui::accesskit::Role::TextInput).type_text("text vi");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    harness.run();
    let app = harness.state().as_ref();
    assert!(app.is_some_and(|a| !a.palette.open), "Enter closes it");
    assert!(app.is_some_and(|a| a.cur.view == View::Text), "and ran Text view");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::P);
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.state().as_ref().is_some_and(|a| !a.palette.open), "Esc closes it");
}

#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn palette_screenshot() {
    let Some(out) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
        let mut harness = egui_kittest::Harness::builder()
            .wgpu()
            .with_size(vec2(1000.0, 560.0))
            .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
        harness.ctx.set_theme(pref);
        harness.run_steps(4);
        harness.key_press_modifiers(Modifiers::COMMAND, Key::P);
        harness.run_steps(3);
        harness.get_by_role(egui::accesskit::Role::TextInput).type_text("sa");
        harness.run_steps(4);
        harness.render().unwrap().save(out.join(format!("palette-{name}.png"))).unwrap();
    }
}

#[test]
fn vim_keys_edit_a_cell() {
    let mut harness = typing_harness();
    if let Some(app) = harness.state_mut() {
        app.vim = true;
    }
    let mode = |h: &egui_kittest::Harness<'_, Option<App>>| h.state().as_ref().map(|a| a.vim_status.clone()).unwrap_or_default();
    harness.run();
    harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("i");
    harness.run();
    assert_eq!(mode(&harness), "INSERT");
    harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("abc def");
    harness.run();
    assert_eq!(source(&harness), "abc def", "insert mode types normally");
    harness.key_press(Key::Escape);
    harness.run();
    assert_eq!(mode(&harness), "NORMAL");
    assert!(code_cell(&harness).is_some());
    harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("0dw");
    harness.run();
    assert_eq!(source(&harness), "def", "0 then dw removes the first word");
    harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("xu");
    harness.run();
    assert_eq!(source(&harness), "def", "x then u restores it");
    harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("Ax");
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    assert_eq!(source(&harness), "defx", "A appends");
    harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text(":q");
    harness.run();
    assert_eq!(mode(&harness), ":q");
    harness.key_press(Key::Escape);
    harness.run();
    assert_eq!(mode(&harness), "NORMAL");
}

#[test]
fn vim_j_scrolls_the_text_view_to_follow_the_cursor() {
    let mut h = egui_kittest::Harness::builder()
        .with_size(vec2(900.0, 400.0))
        .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
    h.run_steps(6);
    if let Some(app) = h.state_mut() {
        app.vim = true;
        let text: String = (0..150).map(|i| format!("# %%\nx{i} = {i}\n")).collect();
        app.replace_notebook(Notebook::from_text(&text), None);
        app.cur.set_view(View::Text);
    }
    h.run_steps(6);
    let editor = |h: &egui_kittest::Harness<'_, Option<App>>| h.get_by_role(egui::accesskit::Role::MultilineTextInput).rect().top();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).focus();
    h.run_steps(6);
    let before = editor(&h);
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text(&"j".repeat(120));
    h.run_steps(4);
    assert!(editor(&h) < before - 200.0, "the view followed the cursor down: {before} -> {}", editor(&h));
}

#[test]
fn undo_and_redo_work_in_a_cell() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("abc");
    h.run();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text(" def");
    h.run();
    assert_eq!(source(&h), "abc def");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(source(&h).len() < 7, "undo removed something: {:?}", source(&h));
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run();
    assert_eq!(source(&h), "abc def");
}

#[test]
fn the_palette_inserts_a_function_into_the_last_cell() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("y = ");
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run();
    h.get_by_role(egui::accesskit::Role::TextInput).type_text("diff(f");
    h.run();
    h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    h.run();
    h.run();
    assert_eq!(source(&h), "y = diff(");
    assert!(!h.state().as_ref().is_some_and(|a| a.palette.open), "the palette closed");
    // Enter alone only opens the function list
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run();
    h.get_by_role(egui::accesskit::Role::TextInput).type_text("integrate(f");
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(source(&h), "y = diff(");
    assert!(h.state().as_ref().is_some_and(|a| a.show_help));
}

#[test]
fn every_symbol_label_has_glyphs_in_the_math_font() {
    // egui panics when a glyph is missing, so check the font file itself
    use skrifa::MetadataProvider;
    let font = skrifa::FontRef::new(fonts::STIX_MATH).unwrap();
    let charmap = font.charmap();
    let missing: Vec<String> = symbols::GROUPS
        .iter()
        .flat_map(|(_, items)| items.iter())
        .flat_map(|t| t.label.chars().filter(|c| charmap.map(*c).is_none()).map(move |c| format!("{c:?} in {:?}", t.label)))
        .collect();
    assert!(missing.is_empty(), "not in STIX Two Math: {missing:?}");
}

#[test]
fn a_symbol_is_inserted_at_the_cursor_with_its_placeholder_selected() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("y = ");
    h.run();
    let ctx = h.ctx.clone();
    if let Some(app) = h.state_mut() {
        app.insert_code(&symbols::Insert::from(&symbols::GROUPS[0].1[0]), &ctx);
    }
    h.run();
    h.run();
    assert_eq!(source(&h), "y = integrate(f, x)");
    // the placeholder is selected, so typing replaces it
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("sin(x)");
    h.run();
    assert_eq!(source(&h), "y = integrate(sin(x), x)");
}

#[test]
fn the_interface_font_has_the_git_summary_symbols() {
    use skrifa::MetadataProvider;
    let font = skrifa::FontRef::new(fonts::INTER).unwrap();
    let charmap = font.charmap();
    for c in ['↑', '↓', '·'] {
        assert!(charmap.map(c).is_some(), "{c:?} is not in the interface font");
    }
}

#[test]
fn keypad_keys_are_typed_at_the_cursor() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("2");
    h.run();
    let ctx = h.ctx.clone();
    if let Some(app) = h.state_mut() {
        for (_, code) in [("^", "**"), ("3", "3")] {
            app.insert_code(&symbols::Insert { code: code.into(), select: String::new() }, &ctx);
        }
    }
    h.run();
    h.run();
    assert_eq!(source(&h), "2**3");
}

#[test]
fn a_matrix_of_the_chosen_size_is_filled_in_with_tab() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("A = ");
    h.run();
    let ctx = h.ctx.clone();
    if let Some(app) = h.state_mut() {
        app.insert_code(&symbols::matrix(2, 2), &ctx);
    }
    h.run();
    h.run();
    assert_eq!(source(&h), "A = [[a11, a12], [a21, a22]]");
    for (i, digit) in ["1", "2", "3", "4"].into_iter().enumerate() {
        if i > 0 {
            h.key_press(Key::Tab);
            h.run();
        }
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text(digit);
        h.run();
    }
    assert_eq!(source(&h), "A = [[1, 2], [3, 4]]");
}

#[test]
fn an_action_wraps_the_expression_on_the_line() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("y = x**2 - 4");
    h.run();
    let ctx = h.ctx.clone();
    let act = |name: &str| symbols::ACTIONS.iter().find(|a| a.func == name).unwrap();
    if let Some(app) = h.state_mut() {
        app.apply_action(act("solve"), &ctx);
    }
    h.run();
    h.run();
    assert_eq!(source(&h), "y = solve(x**2 - 4, x)");
    // the variable is selected, so typing changes it
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("t");
    h.run();
    assert_eq!(source(&h), "y = solve(x**2 - 4, t)");
}

#[test]
fn tab_moves_through_a_templates_placeholders() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("y = ");
    h.run();
    let ctx = h.ctx.clone();
    if let Some(app) = h.state_mut() {
        app.insert_code(&symbols::Insert::from(&symbols::GROUPS[0].1[1]), &ctx);
    }
    h.run();
    h.run();
    assert_eq!(source(&h), "y = integrate(f, (x, a, b))");
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("x**2");
    h.run();
    for (key_text, _) in [("x", 0), ("0", 1), ("3", 2)] {
        h.key_press(Key::Tab);
        h.run();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text(key_text);
        h.run();
    }
    assert_eq!(source(&h), "y = integrate(x**2, (x, 0, 3))");
    // after the last placeholder Tab goes past the template, and then it indents
    h.key_press(Key::Tab);
    h.run();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("+1");
    h.run();
    assert_eq!(source(&h), "y = integrate(x**2, (x, 0, 3))+1");
}

#[test]
fn cmd_alt_arrows_move_between_cells() {
    let mut h = typing_harness();
    if let Some(app) = h.state_mut() {
        app.replace_notebook(Notebook::from_text("# %%\na = 1\n# %% [markdown]\n# text\n# %%\nb = 2\n"), None);
    }
    settle(&mut h);
    h.run();
    let first = h.state().as_ref().and_then(|a| a.cur.nb.cells().first().map(|c| c.id)).unwrap();
    let last = h.state().as_ref().and_then(|a| a.cur.nb.cells().last().map(|c| c.id)).unwrap();
    if let Some(app) = h.state_mut() {
        app.cur.focus = Some(first);
    }
    h.run();
    let focused = |h: &egui_kittest::Harness<'_, Option<App>>| {
        h.state().as_ref().and_then(|a| a.cur.nb.cells().iter().find(|c| h.ctx.memory(|m| m.has_focus(editor_id(c.id)))).map(|c| c.id))
    };
    assert_eq!(focused(&h), Some(first));
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowDown);
    h.run();
    assert_eq!(focused(&h), Some(last), "the text cell in between is skipped");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowUp);
    h.run();
    assert_eq!(focused(&h), Some(first));
}

#[test]
fn long_printed_output_is_folded() {
    let mut h = typing_harness();
    h.set_size(vec2(900.0, 1400.0));
    if let Some(app) = h.state_mut() {
        app.replace_notebook(Notebook::from_text("# %%\nfor i in range(26):\n    print(i)\n# %%\nprint(1)\n"), None);
    }
    settle(&mut h);
    h.run();
    assert!(h.query_by_label("Show 6 more lines").is_some(), "26 lines fold after 20");
    assert_eq!(h.query_all_by_label_contains("Show ").count(), 1, "the one-line output is not folded");
    h.get_by_label("Show 6 more lines").click();
    h.run_steps(4);
    assert!(h.query_by_label("Show less").is_some());
    h.get_by_label("Show less").click();
    h.run_steps(4);
    assert!(h.query_by_label("Show 6 more lines").is_some());
}

#[test]
fn dragging_a_cell_by_its_handle_reorders_it() {
    let mut h = typing_harness();
    h.set_size(vec2(900.0, 900.0));
    let ids = {
        let app = h.state_mut().as_mut().unwrap();
        app.replace_notebook(Notebook::from_text("# %%\na = 1\n# %%\nb = 2\n# %%\nc = 3\n"), None);
        app.cur.nb.cells().iter().map(|c| c.id).collect::<Vec<_>>()
    };
    settle(&mut h);
    h.run();
    let editors: Vec<_> = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).map(|n| n.rect()).collect();
    assert_eq!(editors.len(), 3);
    // point at the first cell so its tools show, then drag its handle down past the second cell's middle
    h.hover_at(editors[0].center());
    h.run_steps(3);
    let grip = h.get_by_label("Drag to move this cell").rect().center();
    h.hover_at(grip);
    h.run_steps(2);
    h.drag_at(grip);
    h.run_steps(2);
    let mut y = grip.y;
    while y < editors[1].center().y + 20.0 {
        y += 10.0;
        h.hover_at(egui::pos2(grip.x, y));
        h.run_steps(2);
    }
    h.drop_at(egui::pos2(grip.x, y));
    h.run_steps(3);
    let order: Vec<_> = h.state().as_ref().unwrap().cur.nb.cells().iter().map(|c| c.id).collect();
    assert_eq!(order, vec![ids[1], ids[0], ids[2]], "the first cell moved below the second");
    // one drag is one undo step
    let app = h.state_mut().as_mut().unwrap();
    app.undo_cells();
    let order: Vec<_> = app.cur.nb.cells().iter().map(|c| c.id).collect();
    assert_eq!(order, ids);
}

#[test]
fn pointing_at_a_function_shows_its_signature() {
    let mut h = typing_harness();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("y = diff(x, x)");
    h.run();
    let rect = h.get_by_role(egui::accesskit::Role::MultilineTextInput).rect();
    // two characters into the function name, vertically in the first row
    let font = 8.0;
    h.hover_at(egui::pos2(rect.left() + EDITOR_PAD_X + font * 6.5, rect.top() + 14.0));
    for _ in 0..12 {
        h.run_steps(2);
    }
    assert!(h.query_by_label_contains("diff(f, x[, n])").is_some(), "the tooltip names the signature");
    // away from any name: nothing
    h.hover_at(egui::pos2(rect.right() - 20.0, rect.top() + 14.0));
    for _ in 0..4 {
        h.run_steps(2);
    }
    assert!(h.query_by_label_contains("diff(f, x[, n])").is_none());
}

#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn completion_screenshot() {
    let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
        let mut h = egui_kittest::Harness::builder()
            .wgpu()
            .with_size(vec2(900.0, 420.0))
            .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
        h.ctx.set_theme(pref);
        h.run();
        if let Some(app) = h.state_mut() {
            app.replace_notebook(Notebook::from_text("# %%\nvelocity = 3\n# %%\n"), None);
        }
        h.run();
        let editors = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).count();
        assert!(editors >= 2);
        h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).nth(1).unwrap().focus();
        h.run();
        h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).nth(1).unwrap().type_text("y = ex");
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("completion-{name}.png"))).unwrap();
    }
}

#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn narrow_screenshot() {
    let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = egui_kittest::Harness::builder()
        .wgpu()
        .with_size(vec2(480.0, 600.0))
        .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
    h.ctx.set_theme(ThemePreference::Light);
    h.run_steps(4);
    if let Some(app) = h.state_mut() {
        app.show_help = true;
    }
    h.run_steps(6);
    h.render().unwrap().save(dir.join("narrow-help.png")).unwrap();
}

#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn menu_screenshot() {
    let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    for (name, pref) in [("light", ThemePreference::Light), ("dark", ThemePreference::Dark)] {
        for menu in ["File", "Edit", "Settings"] {
            let mut h = egui_kittest::Harness::builder()
                .wgpu()
                .with_size(vec2(760.0, 640.0))
                .with_pixels_per_point(2.0)
                .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
            h.ctx.set_theme(pref);
            h.run_steps(4);
            h.get_by_label(menu).click();
            h.run_steps(6);
            h.render().unwrap().save(dir.join(format!("menu-{menu}-{name}.png"))).unwrap();
        }
    }
}

#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn symbols_screenshot() {
    let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = egui_kittest::Harness::builder()
        .wgpu()
        .with_size(vec2(900.0, 620.0))
        .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
    h.ctx.set_theme(ThemePreference::Light);
    h.run_steps(4);
    if let Some(app) = h.state_mut() {
        app.replace_notebook(
            Notebook::from_text(
                "# %%
y = diff(f(x), x)
",
            ),
            None,
        );
        app.show_symbols = true;
    }
    h.run_steps(6);
    h.render().unwrap().save(dir.join("symbols.png")).unwrap();
}

#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn powers_screenshot() {
    let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = egui_kittest::Harness::builder()
        .wgpu()
        .with_size(vec2(760.0, 300.0))
        .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
    h.ctx.set_theme(ThemePreference::Light);
    h.run_steps(4);
    if let Some(app) = h.state_mut() {
        app.replace_notebook(Notebook::from_text("# %%\nf = x**2 + 3*y^3 - (a+b)**(n+1)\ndiff(f, x) / e^-x + 2**10\n"), None);
    }
    h.run_steps(6);
    h.render().unwrap().save(dir.join("powers.png")).unwrap();
}

/// `MATHCRAFT_SHOTS=<dir>` is set: `MATHCRAFT_SHOTS=/tmp/shots cargo test -p mathcraft screenshots -- --ignored`.
#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn screenshots() {
    let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    // `MATHCRAFT_NOTEBOOK=<file>` renders that notebook instead of the welcome one.
    let file = std::env::var_os("MATHCRAFT_NOTEBOOK").map(PathBuf::from);
    for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
        for (suffix, size, ppp) in [
            ("top", vec2(1060.0, 900.0), 2.0),
            (
                "full",
                vec2(
                    std::env::var("MATHCRAFT_WIDTH").ok().and_then(|w| w.parse().ok()).unwrap_or(1060.0),
                    std::env::var("MATHCRAFT_HEIGHT").ok().and_then(|h| h.parse().ok()).unwrap_or(if file.is_some() { 1100.0 } else { 3000.0 }),
                ),
                1.0,
            ),
        ] {
            let mut harness = egui_kittest::Harness::builder()
                .wgpu()
                .with_size(size)
                .with_pixels_per_point(ppp)
                .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), file.clone())).show(ui), None);
            harness.ctx.set_theme(pref);
            let start = Instant::now();
            loop {
                harness.run_steps(3);
                std::thread::sleep(Duration::from_millis(30));
                if harness.state().as_ref().is_some_and(|a| !a.busy()) || start.elapsed() > Duration::from_secs(60) {
                    break;
                }
            }
            harness.run_steps(6);
            let img = harness.render().unwrap();
            img.save(dir.join(format!("{name}-{suffix}.png"))).unwrap();
        }
    }
}

/// Three open documents, so the tab strip shows.
#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn tabs_screenshot() {
    let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
        let mut harness = egui_kittest::Harness::builder().wgpu().with_size(vec2(1000.0, 520.0)).build_ui_state(
            |ui, app: &mut Option<App>| {
                app.get_or_insert_with(|| {
                    let mut app = App::with_context(ui.ctx().clone(), None);
                    app.open_in_tab(Notebook::from_text("# %%\nf = sin(x)**2\n# %%\ndiff(f, x)\n"), Some(PathBuf::from("/tmp/derivatives.py")));
                    app.open_in_tab(Notebook::from_text("# %%\nsolve(x**2 - 2, x)\n"), Some(PathBuf::from("/tmp/a_rather_long_notebook_name_here.py")));
                    app.cur.dirty = true;
                    // lay the toolbar out as the macOS title bar (the window buttons themselves are native, not drawn)
                    app.unified_title_bar = true;
                    app
                })
                .show(ui)
            },
            None,
        );
        harness.ctx.set_theme(pref);
        let start = Instant::now();
        while harness.state().as_ref().is_none_or(App::busy) && start.elapsed() < Duration::from_secs(30) {
            harness.run_steps(3);
            std::thread::sleep(Duration::from_millis(30));
        }
        harness.run_steps(6);
        harness.render().unwrap().save(dir.join(format!("tabs-{name}.png"))).unwrap();
    }
}

/// A new notebook (the empty state) with the function list open, in both themes.
#[test]
#[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
fn help_screenshot() {
    let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
        let mut h = egui_kittest::Harness::builder()
            .wgpu()
            .with_size(vec2(1000.0, 720.0))
            .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
        h.ctx.set_theme(pref);
        h.run();
        if let Some(app) = h.state_mut() {
            app.replace_notebook(Notebook::from_text("# %%\n"), None);
        }
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("empty-{name}.png"))).unwrap();
        if let Some(app) = h.state_mut() {
            app.show_help = true;
        }
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("help-{name}.png"))).unwrap();
    }
}

fn run_to_idle(ctx: &egui::Context, app: &mut App) {
    let start = Instant::now();
    while app.busy() || start.elapsed() < Duration::from_millis(150) {
        frame(ctx, app);
        std::thread::sleep(Duration::from_millis(10));
    }
    frame(ctx, app);
}

fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("mathcraft-app-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (cfg, work) = (base.join("cfg"), base.join("work"));
    std::fs::create_dir_all(&work).unwrap();
    (cfg, work)
}

#[test]
fn results_are_shown_again_when_a_notebook_is_reopened_and_only_new_cells_run() {
    let (cfg, work) = scratch("results");
    let file = work.join("a.py");
    std::fs::write(&file, "# %%\ny = x**2 + 1\n\n# %%\ndiff(y, x)\n").unwrap();
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.load_settings(Some(cfg.clone()));
    app.open_path(file.clone());
    run_to_idle(&ctx, &mut app);
    assert!(app.cur.nb.cells().iter().all(|c| c.state == CellState::Fresh));

    // a new launch shows the same results, dimmed, without running anything
    let ctx2 = egui::Context::default();
    let mut again = App::with_context(ctx2.clone(), None);
    again.load_settings(Some(cfg.clone()));
    again.open_path(file.clone());
    assert!(!again.cur.busy(), "nothing is queued: every cell has a saved result");
    let cells = again.cur.nb.cells();
    assert!(cells.iter().all(|c| c.state == CellState::Stale && c.output.is_some()));
    assert!(cells.get(1).and_then(|c| c.output.as_ref()).is_some_and(|o| format!("{:?}", o.result).contains("2*x")));
    frame(&ctx2, &mut again);

    // running one cell brings its inputs up to date first
    let id = again.cur.nb.cells().get(1).map(|c| c.id).unwrap_or(0);
    again.run(RunRequest::Cell(id));
    run_to_idle(&ctx2, &mut again);
    assert!(again.cur.nb.cells().iter().all(|c| c.state == CellState::Fresh));

    // editing the file outside makes the results of the changed cell and those below it run again
    std::fs::write(&file, "# %%\ny = x**3\n\n# %%\ndiff(y, x)\n").unwrap();
    let mut edited = App::with_context(egui::Context::default(), None);
    edited.load_settings(Some(cfg.clone()));
    edited.open_path(file);
    assert!(edited.cur.busy(), "the changed cells run");
    let _ = std::fs::remove_dir_all(cfg.parent().unwrap_or(&cfg));
}

#[test]
fn an_untitled_notebook_comes_back_with_its_results_and_goes_when_closed_or_saved() {
    let (cfg, _) = scratch("drafts");
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.load_settings(Some(cfg.clone()));
    app.replace_notebook(Notebook::from_text("# %%\nz = 3\nz * 7\n"), None);
    app.cur.dirty = true;
    run_to_idle(&ctx, &mut app);
    app.persist_session(true);
    let name = app.cur.draft.clone().expect("typed-into notebooks become drafts");
    assert!(store::read_draft(&cfg, &name).is_some_and(|t| t.contains("z * 7")));
    assert!(!app.any_dirty(), "quitting does not discard it");

    let mut again = App::with_context(egui::Context::default(), None);
    again.load_settings(Some(cfg.clone()));
    again.load_session(false);
    assert_eq!(again.cur.draft.as_deref(), Some(name.as_str()));
    assert!(again.cur.dirty && again.cur.path.is_none());
    assert!(!again.cur.busy());
    assert!(again.cur.nb.cells().first().is_some_and(|c| c.state == CellState::Stale && c.output.is_some()));

    // closing the tab throws the draft away (the confirmation dialog is for unsaved changes, so say there are none)
    again.cur.dirty = false;
    let id = again.cur.id;
    again.close_tab(id);
    assert_eq!(store::read_draft(&cfg, &name), None);
    let _ = std::fs::remove_dir_all(cfg.parent().unwrap_or(&cfg));
}

#[test]
fn scroll_and_cursor_are_remembered_per_tab() {
    let (cfg, work) = scratch("view");
    let file = work.join("v.py");
    std::fs::write(&file, "# %%\na = 1\n\n# %%\nbb = 22\n").unwrap();
    let ctx = egui::Context::default();
    let mut app = App::with_context(ctx.clone(), None);
    app.load_settings(Some(cfg.clone()));
    app.open_path(file.clone());
    app.cur.memo = session::Memo { scroll: 37, cursor: Some((1, 3)) };
    app.persist_session(true);

    let mut again = App::with_context(egui::Context::default(), None);
    again.load_settings(Some(cfg.clone()));
    again.load_session(false);
    assert_eq!(again.cur.restore_view, Some(session::Memo { scroll: 37, cursor: Some((1, 3)) }));
    assert_eq!(again.current_session().views.get(&file.to_string_lossy().into_owned()).map(|m| m.scroll), Some(37), "a tab not yet shown keeps its view");
    let ctx2 = egui::Context::default();
    let scroll = again.take_restored_view(&ctx2);
    assert_eq!(scroll, Some(37.0));
    let second = again.cur.nb.cells().get(1).map(|c| c.id);
    assert_eq!(again.cur.focus, second, "the cell that had the cursor takes the keyboard");
    let caret = egui::TextEdit::load_state(&ctx2, editor_id(second.unwrap_or(0))).and_then(|s| s.cursor.char_range()).map(|r| usize::from(r.primary.index));
    assert_eq!(caret, Some(3));
    assert!(again.cur.restore_view.is_none(), "restored once");
    let _ = std::fs::remove_dir_all(cfg.parent().unwrap_or(&cfg));
}
