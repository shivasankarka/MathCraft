//! Saved results: the last output of each cell as JSON, so a notebook can show them again before anything runs.
//!
//! A saved result belongs to a cell only while that cell and everything it reads from are unchanged, which a
//! fingerprint of their source decides. Values are stored as the text they print as and evaluated again on load
//! (under a small budget), and kept only when that gives the same text back; plots, functions and anything that does
//! not survive the round trip are left out, so those cells simply run when the notebook opens.

use crate::notebook::{CellKind, CellOutput, CellState, Notebook};
use mathcraft_core::budget::{self, Budget};
use mathcraft_eval::{EvalError, Value};
use serde_json::{Map, Value as Json, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const VERSION: u64 = 1;
/// Longer results are not saved: they are slow to read back and rarely worth showing before they are recomputed.
const MAX_TEXT: usize = 8_000;
/// What reading back one value may cost.
const RESTORE_TIME: Duration = Duration::from_millis(100);
const RESTORE_OPS: u64 = 2_000_000;

fn fnv(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= u64::from(*b);
        *h = h.wrapping_mul(0x0100_0000_01b3);
    }
    // a separator, so ("ab", "c") and ("a", "bc") differ
    *h ^= 0xff;
    *h = h.wrapping_mul(0x0100_0000_01b3);
}

/// Can this value be written as text and read back?
fn restorable(v: &Value) -> bool {
    match v {
        Value::Expr(_) | Value::Bool(_) | Value::None | Value::Range { .. } | Value::Array(_) => true,
        Value::List(items) | Value::Tuple(items) => items.iter().all(restorable),
        Value::Str(_) | Value::Func(_) | Value::Builtin(_) | Value::Plot(_) | Value::DType(_) => false,
    }
}

/// Read a printed value back; `None` unless it prints as exactly the same text again.
fn read_back(text: &str) -> Option<Value> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let _guard = budget::install(Budget::new(Some(RESTORE_TIME), Arc::new(AtomicBool::new(false)), RESTORE_OPS));
    let out = mathcraft_eval::run_source(&mut HashMap::new(), text, false);
    match out.result {
        Ok(Some(v)) if out.stdout.is_empty() && restorable(&v) && v.to_string() == text => Some(v),
        _ => None,
    }
}

impl Notebook {
    /// One fingerprint per cell: its source together with the source of every cell it reads from, directly or not.
    pub fn fingerprints(&self) -> Vec<u64> {
        (0..self.cells.len())
            .map(|i| {
                let mut h = 0xcbf2_9ce4_8422_2325_u64;
                let mut chain = self.ancestors_of(i);
                chain.sort_unstable();
                for a in chain.into_iter().chain(std::iter::once(i)) {
                    if let Some(c) = self.cells.get(a) {
                        fnv(&mut h, &[u8::from(c.kind == CellKind::Code)]);
                        fnv(&mut h, c.source.as_bytes());
                    }
                }
                h
            })
            .collect()
    }

    /// The results that can be shown again later, as JSON.
    pub fn export_outputs(&self) -> String {
        let mut entries = Vec::new();
        for (c, fp) in self.cells.iter().zip(self.fingerprints()) {
            let Some(o) = &c.output else { continue };
            // a stale result is kept only while it is still the one that was restored for this very source
            let kept = match c.state {
                CellState::Fresh | CellState::Error => true,
                CellState::Stale => c.restored_fp == Some(fp),
                _ => false,
            };
            if c.kind != CellKind::Code || !kept {
                continue;
            }
            let mut e = Map::new();
            e.insert("fp".into(), json!(format!("{fp:016x}")));
            e.insert("stdout".into(), json!(o.stdout));
            e.insert("ms".into(), json!(u64::try_from(o.elapsed.as_millis()).unwrap_or(u64::MAX)));
            match &o.result {
                Ok(None) => {}
                Ok(Some(v)) if restorable(v) && v.to_string().len() <= MAX_TEXT => {
                    e.insert("value".into(), json!(v.to_string()));
                }
                Ok(Some(_)) => continue,
                Err(err) => {
                    e.insert("error".into(), json!({ "line": err.line, "message": err.message }));
                }
            }
            entries.push(Json::Object(e));
        }
        serde_json::to_string(&json!({ "version": VERSION, "cells": entries })).unwrap_or_default()
    }

    /// Show saved results on the cells they still belong to, dimmed as stale until the cell runs again. Returns how
    /// many cells got one. Bad or outdated data restores nothing.
    pub fn import_outputs(&mut self, json: &str) -> usize {
        let Ok(Json::Object(root)) = serde_json::from_str::<Json>(json) else { return 0 };
        if root.get("version").and_then(Json::as_u64) != Some(VERSION) {
            return 0;
        }
        let Some(Json::Array(entries)) = root.get("cells") else { return 0 };
        let by_fp: HashMap<&str, &Json> = entries.iter().filter_map(|e| Some((e.get("fp")?.as_str()?, e))).collect();
        let mut restored = 0;
        for (i, fp) in self.fingerprints().into_iter().enumerate() {
            let Some(entry) = by_fp.get(format!("{fp:016x}").as_str()) else { continue };
            let Some(cell) = self.cells.get_mut(i) else { continue };
            if cell.kind != CellKind::Code || cell.state != CellState::New || !cell.problems.is_empty() {
                continue;
            }
            let result = match (entry.get("value").and_then(Json::as_str), entry.get("error")) {
                (Some(text), _) => match read_back(text) {
                    Some(v) => Ok(Some(v)),
                    None => continue,
                },
                (None, Some(err)) => Err(EvalError {
                    line: err.get("line").and_then(Json::as_u64).and_then(|n| u32::try_from(n).ok()),
                    message: err.get("message").and_then(Json::as_str).unwrap_or_default().to_owned(),
                }),
                (None, None) => Ok(None),
            };
            let stdout = entry.get("stdout").and_then(Json::as_str).unwrap_or_default().to_owned();
            let elapsed = Duration::from_millis(entry.get("ms").and_then(Json::as_u64).unwrap_or(0));
            cell.output = Some(CellOutput { stdout, result, elapsed });
            cell.state = CellState::Stale;
            cell.restored_fp = Some(fp);
            restored += 1;
        }
        restored
    }
}
