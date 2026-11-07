//! Static analysis of a cell, without running it: which global names it defines and which it reads from elsewhere.
//!
//! - Definitions are names assigned at the cell's top level (including inside `if`/`for`/`while` blocks, as in Python).
//! - References are names read but not defined earlier in the same cell. Inside functions and lambdas a name that is
//!   not local is a free variable looked up when the function is *called*, so it counts as a reference unless the
//!   cell itself defines it.
//! - Names starting with `_` are private to the cell: they never create dependencies.

use mathcraft_syntax::ast::{Comp, Ex, Param, Stmt, StmtKind, Target};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellAnalysis {
    pub defs: BTreeSet<String>,
    pub refs: BTreeSet<String>,
}

pub fn analyze(stmts: &[Stmt]) -> CellAnalysis {
    let mut all_defs = BTreeSet::new();
    collect_defs(stmts, &mut all_defs);
    let mut w = Walker { all_defs, defined: BTreeSet::new(), refs: BTreeSet::new(), scopes: Vec::new(), deferred: 0 };
    w.stmts(stmts);
    CellAnalysis { defs: w.all_defs.into_iter().filter(|n| !is_private(n)).collect(), refs: w.refs.into_iter().filter(|n| !is_private(n)).collect() }
}

fn is_private(name: &str) -> bool {
    name.starts_with('_')
}

fn target_names(t: &Target, out: &mut BTreeSet<String>) {
    let mut v = Vec::new();
    t.names(&mut v);
    out.extend(v);
}

/// Names bound by a block of statements (not descending into function bodies).
fn collect_defs(stmts: &[Stmt], out: &mut BTreeSet<String>) {
    for s in stmts {
        match &s.kind {
            StmtKind::Assign { targets, .. } => targets.iter().for_each(|t| target_names(t, out)),
            StmtKind::AugAssign { target, .. } => target_names(target, out),
            StmtKind::Def { name, .. } => {
                out.insert(name.clone());
            }
            StmtKind::For { target, body, .. } => {
                target_names(target, out);
                collect_defs(body, out);
            }
            StmtKind::If { body, orelse, .. } => {
                collect_defs(body, out);
                collect_defs(orelse, out);
            }
            StmtKind::While { body, .. } => collect_defs(body, out),
            StmtKind::Expr(_) | StmtKind::Return(_) | StmtKind::Pass | StmtKind::Break | StmtKind::Continue => {}
        }
    }
}

struct Walker {
    all_defs: BTreeSet<String>,
    /// Top-level names already defined at this point of the cell.
    defined: BTreeSet<String>,
    refs: BTreeSet<String>,
    scopes: Vec<BTreeSet<String>>,
    /// How many enclosing function bodies or lambdas (their code runs later, when called).
    deferred: usize,
}

impl Walker {
    fn load(&mut self, name: &str) {
        if self.scopes.iter().any(|s| s.contains(name)) {
            return;
        }
        let internal = if self.deferred > 0 { self.all_defs.contains(name) } else { self.defined.contains(name) };
        if !internal {
            self.refs.insert(name.to_string());
        }
    }

    fn bind(&mut self, name: &str) {
        match self.scopes.last_mut() {
            Some(s) => {
                s.insert(name.to_string());
            }
            None => {
                self.defined.insert(name.to_string());
            }
        }
    }

    fn bind_target(&mut self, t: &Target) {
        let mut names = Vec::new();
        t.names(&mut names);
        names.iter().for_each(|n| self.bind(n));
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        stmts.iter().for_each(|s| self.stmt(s));
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::Assign { targets, value } => {
                self.expr(value);
                targets.iter().for_each(|t| self.bind_target(t));
            }
            StmtKind::AugAssign { target, value, .. } => {
                self.expr(value);
                let mut names = Vec::new();
                target.names(&mut names);
                names.iter().for_each(|n| self.load(n));
                self.bind_target(target);
            }
            StmtKind::Def { name, params, body } => {
                self.bind(name);
                self.function(params, |w| w.stmts(body), Some(body));
            }
            StmtKind::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            StmtKind::If { test, body, orelse } => {
                self.expr(test);
                self.stmts(body);
                self.stmts(orelse);
            }
            StmtKind::For { target, iter, body } => {
                self.expr(iter);
                self.bind_target(target);
                self.stmts(body);
            }
            StmtKind::While { test, body } => {
                self.expr(test);
                self.stmts(body);
            }
            StmtKind::Pass | StmtKind::Break | StmtKind::Continue => {}
        }
    }

    /// Analyse a function or lambda body in its own scope: parameters and (for `def`) assigned names are local.
    fn function(&mut self, params: &[Param], body: impl FnOnce(&mut Walker), block: Option<&[Stmt]>) {
        for p in params {
            if let Some(d) = &p.default {
                self.expr(d);
            }
        }
        let mut scope: BTreeSet<String> = params.iter().map(|p| p.name.clone()).collect();
        if let Some(b) = block {
            collect_defs(b, &mut scope);
        }
        self.scopes.push(scope);
        self.deferred += 1;
        body(self);
        self.deferred -= 1;
        self.scopes.pop();
    }

    fn expr(&mut self, e: &Ex) {
        match e {
            Ex::Name(n) => self.load(n),
            Ex::Int(_) | Ex::Float(_) | Ex::Str(_) | Ex::Bool(_) | Ex::None => {}
            Ex::Bin(a, _, b) => {
                self.expr(a);
                self.expr(b);
            }
            Ex::Unary(_, x) | Ex::Attr(x, _) => self.expr(x),
            Ex::Compare(first, rest) => {
                self.expr(first);
                rest.iter().for_each(|(_, x)| self.expr(x));
            }
            Ex::And(xs) | Ex::Or(xs) | Ex::List(xs) | Ex::Tuple(xs) => xs.iter().for_each(|x| self.expr(x)),
            Ex::Call { func, args, kwargs } => {
                self.expr(func);
                args.iter().for_each(|x| self.expr(x));
                kwargs.iter().for_each(|(_, x)| self.expr(x));
            }
            Ex::Index(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Ex::Lambda { params, body } => self.function(params, |w| w.expr(body), None),
            Ex::IfExp { test, body, orelse } => {
                self.expr(test);
                self.expr(body);
                self.expr(orelse);
            }
            Ex::ListComp { elt, gens } => {
                self.scopes.push(BTreeSet::new());
                for Comp { target, iter, ifs } in gens {
                    self.expr(iter);
                    self.bind_target(target);
                    ifs.iter().for_each(|x| self.expr(x));
                }
                self.expr(elt);
                self.scopes.pop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(src: &str) -> (Vec<String>, Vec<String>) {
        let r = analyze(&mathcraft_syntax::parse(src).unwrap());
        (r.defs.into_iter().collect(), r.refs.into_iter().collect())
    }

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn simple_assignment() {
        assert_eq!(a("y = x + 1"), (v(&["y"]), v(&["x"])));
    }

    #[test]
    fn definitions_before_use_are_internal() {
        assert_eq!(a("a = 1\nb = a + c"), (v(&["a", "b"]), v(&["c"])));
    }

    #[test]
    fn functions_look_up_free_variables_at_call_time() {
        assert_eq!(a("def f(t):\n    return t * k + helper(t)\n"), (v(&["f"]), v(&["helper", "k"])));
        // recursion and later-defined helpers in the same cell are internal
        assert_eq!(a("def f(n):\n    return g(n) + f(n - 1)\ndef g(n):\n    return n\n"), (v(&["f", "g"]), v(&[])));
    }

    #[test]
    fn locals_do_not_leak() {
        assert_eq!(a("def f(x):\n    y = x + 1\n    return y\n"), (v(&["f"]), v(&[])));
        // builtins like `range` show up as references; the graph ignores names no cell defines
        assert_eq!(a("r = [i * m for i in range(3)]"), (v(&["r"]), v(&["m", "range"])));
        assert_eq!(a("g = lambda t: t + s"), (v(&["g"]), v(&["s"])));
    }

    #[test]
    fn loops_and_unpacking_define_names() {
        assert_eq!(a("for i in rng:\n    total = i"), (v(&["i", "total"]), v(&["rng"])));
        assert_eq!(a("p, q = pair"), (v(&["p", "q"]), v(&["pair"])));
    }

    #[test]
    fn augmented_assignment_reads_first() {
        assert_eq!(a("n += 1"), (v(&["n"]), v(&["n"])));
        assert_eq!(a("n = 0\nn += 1"), (v(&["n"]), v(&[])));
    }

    #[test]
    fn private_names_are_cell_local() {
        assert_eq!(a("_tmp = x\ny = _tmp * 2"), (v(&["y"]), v(&["x"])));
    }

    #[test]
    fn function_calls_reference_the_function_name() {
        assert_eq!(a("diff(f, x)"), (v(&[]), v(&["diff", "f", "x"])));
    }
}
