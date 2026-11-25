use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use xls42_core::{SolverResult, SolverTask, Workbook};
pub const START: u16 = 82;
pub const CANCEL: u16 = 83;
pub const RESULT: u16 = 84;
struct Worker {
    rx: mpsc::Receiver<Result<SolverResult, String>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
#[derive(Default)]
pub struct Session {
    worker: Option<Worker>,
    outcome: Option<Result<SolverResult, String>>,
    ready: bool,
}
impl Session {
    pub fn running(&self) -> bool {
        self.worker.is_some()
    }
    pub fn has_outcome(&self) -> bool {
        self.outcome.is_some()
    }
    pub fn start(&mut self, task: SolverTask) -> Result<(), String> {
        self.cancel();
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("off42-solver".into())
            .spawn(move || {
                let _ = tx.send(task.solve(&signal));
            })
            .map_err(|error| error.to_string())?;
        self.worker = Some(Worker { rx, cancel });
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.worker = None;
        self.outcome = None;
        self.ready = false;
    }
    pub fn poll(&mut self, book: &mut Workbook) -> bool {
        let result = match self.worker.as_ref().map(|worker| worker.rx.try_recv()) {
            Some(Ok(result)) => Some(result),
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                Some(Err("Solver stopped unexpectedly".into()))
            }
            _ => None,
        };
        if let Some(mut result) = result {
            if let Ok(outcome) = &result {
                if !outcome.can_keep() {
                    if let Err(error) = book.apply_solver_result(outcome, false) {
                        result = Err(error.to_string());
                    }
                }
            }
            self.worker = None;
            self.outcome = Some(result);
            self.ready = true;
            true
        } else {
            false
        }
    }
    pub fn take_command(&mut self) -> Option<u16> {
        std::mem::take(&mut self.ready).then_some(RESULT)
    }
    pub fn dialog(&self) -> Option<win42::DialogRequest> {
        Some(match self.outcome.as_ref()? {
            Ok(result) if result.can_keep() => win42::DialogRequest::Confirmation {
                title: "Solver found a solution",
                text: format!(
                    "{} = {}\n{}\n{}\n\n{}",
                    crate::goal_seek::reference(result.model().objective),
                    result.objective().unwrap(),
                    win42::localization::text(
                        if result.status() == xls42_core::SolverStatus::Approximate {
                            "Approximate solution"
                        } else {
                            "Linear (simplex)"
                        }
                    ),
                    result
                        .model()
                        .variables
                        .iter()
                        .zip(result.values())
                        .map(|(&cell, value)| format!(
                            "{} = {}",
                            crate::goal_seek::reference(cell),
                            value
                        ))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    win42::localization::text("%@ = %@. Keep the result?")
                        .replacen(
                            "%@",
                            &crate::goal_seek::reference(result.model().objective),
                            1
                        )
                        .replacen("%@", &result.objective().unwrap().to_string(), 1)
                ),
            },
            Ok(result) => win42::DialogRequest::Information {
                title: "Solver",
                text: win42::localization::text(match result.status() {
                    xls42_core::SolverStatus::Infeasible => {
                        "No solution satisfies all the conditions."
                    }
                    xls42_core::SolverStatus::Unbounded => {
                        "The result can grow without limit (unbounded)."
                    }
                    _ => "The model can't be solved: check the cells and conditions.",
                })
                .into(),
            },
            Err(error) => win42::DialogRequest::Information {
                title: "Solver",
                text: error.clone(),
            },
        })
    }
    pub fn accept(&mut self, book: &mut Workbook, keep: bool) -> Result<(), String> {
        let result = self.outcome.take();
        self.ready = false;
        let result = result.ok_or("Solver result is unavailable")??;
        book.apply_solver_result(&result, keep)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}
use crate::goal_seek::{parse_reference, reference};
use xls42_core::{Range, SolverConstraint, SolverGoal, SolverModel, SolverOp};
fn range_text(r: Range) -> String {
    let a = reference((r.top, r.left));
    if r.top == r.bottom && r.left == r.right {
        a
    } else {
        format!("{a}:{}", reference((r.bottom, r.right)))
    }
}
fn range(text: &str, book: &Workbook) -> Result<Range, String> {
    let mut p = text.trim().split(':');
    let a = parse_reference(p.next().unwrap_or(""), book)?;
    let b = p
        .next()
        .map(|v| parse_reference(v, book))
        .transpose()?
        .unwrap_or(a);
    if p.next().is_some() || a.0 > b.0 || a.1 > b.1 {
        return Err("Enter a cell or range such as A1:B3".into());
    }
    Ok(Range {
        top: a.0,
        left: a.1,
        bottom: b.0,
        right: b.1,
    })
}
pub fn request(book: &Workbook, cell: (usize, usize)) -> win42::InputRequest {
    let saved = book.solver_model();
    let goal = match saved.map(|m| &m.goal) {
        Some(SolverGoal::Minimum) => "Minimum",
        Some(SolverGoal::Value(_)) => "Value of",
        _ => "Maximum",
    };
    win42::InputRequest {
        title: "Solver",
        fields: vec![
            (
                "Target cell:",
                reference(saved.map(|m| m.objective).unwrap_or(cell)),
            ),
            ("Optimize result to:", goal.into()),
            (
                "Target value:",
                saved
                    .and_then(|m| {
                        if let SolverGoal::Value(v) = m.goal {
                            Some(v.to_string())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "0".into()),
            ),
            (
                "By changing cells:",
                saved
                    .map(|m| {
                        m.variables
                            .iter()
                            .map(|&c| reference(c))
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_default(),
            ),
            (
                "Limiting conditions (one per line: A1 <= 10; B1 integer; C1 binary):",
                saved
                    .map(|m| {
                        m.constraints
                            .iter()
                            .map(|c| {
                                format!(
                                    "{} {} {}",
                                    range_text(c.lhs),
                                    match c.op {
                                        SolverOp::Le => "<=",
                                        SolverOp::Ge => ">=",
                                        SolverOp::Eq => "=",
                                        SolverOp::Integer => "integer",
                                        SolverOp::Binary => "binary",
                                    },
                                    c.rhs
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\r\n")
                    })
                    .unwrap_or_default(),
            ),
            (
                "Assume non-negative variables",
                if saved.is_none_or(|m| m.non_negative) {
                    "Yes"
                } else {
                    "No"
                }
                .into(),
            ),
            (
                "Solving method:",
                if saved.is_none_or(|m| m.linear) {
                    "Linear (simplex)"
                } else {
                    "Nonlinear (Nelder–Mead)"
                }
                .into(),
            ),
        ],
    }
}
pub fn choices() -> Vec<(usize, Vec<String>)> {
    vec![
        (
            1,
            vec!["Maximum".into(), "Minimum".into(), "Value of".into()],
        ),
        (5, vec!["Yes".into(), "No".into()]),
        (
            6,
            vec!["Linear (simplex)".into(), "Nonlinear (Nelder–Mead)".into()],
        ),
    ]
}
pub fn parse(v: &[String], book: &Workbook) -> Result<SolverModel, String> {
    if v.len() != 7 {
        return Err("Invalid Solver model".into());
    }
    let objective = parse_reference(&v[0], book)?;
    let goal = match v[1].as_str() {
        "Maximum" => SolverGoal::Maximum,
        "Minimum" => SolverGoal::Minimum,
        "Value of" => SolverGoal::Value(
            v[2].trim()
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or("Enter a finite target value")?,
        ),
        _ => return Err("Choose Maximum, Minimum, or Value of".into()),
    };
    let mut variables = Vec::new();
    for part in v[3]
        .split(|c: char| c == ';' || c == ',' || c.is_ascii_whitespace())
        .filter(|s| !s.is_empty())
    {
        let r = range(part, book)?;
        let n = (r.bottom - r.top + 1) * (r.right - r.left + 1);
        if variables.len() + n > 32 {
            return Err("Choose up to 32 variable cells".into());
        }
        for row in r.top..=r.bottom {
            for col in r.left..=r.right {
                variables.push((row, col));
            }
        }
    }
    if variables.is_empty() {
        return Err("Choose variable cells".into());
    }
    let mut constraints = Vec::new();
    for line in v[4]
        .split(['\n', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if constraints.len() >= 128 {
            return Err("Choose up to 128 constraints".into());
        }
        let lower = line.to_ascii_lowercase();
        let mut found = None;
        for (token, op) in [
            ("<=", SolverOp::Le),
            (">=", SolverOp::Ge),
            ("≤", SolverOp::Le),
            ("≥", SolverOp::Ge),
            ("=", SolverOp::Eq),
            ("integer", SolverOp::Integer),
            ("binary", SolverOp::Binary),
        ] {
            if let Some(i) = lower.find(token) {
                found = Some((i, token.len(), op));
                break;
            }
        }
        let (at, len, op) = found.ok_or("Use <=, >=, =, integer, or binary in each constraint")?;
        let lhs = range(&line[..at], book)?;
        let rhs = line[at + len..].trim().to_owned();
        if matches!(op, SolverOp::Integer | SolverOp::Binary) && !rhs.is_empty() {
            return Err("Integer and binary constraints have no right-hand value".into());
        }
        constraints.push(SolverConstraint { lhs, op, rhs });
    }
    let non_negative = match v[5].as_str() {
        "Yes" => true,
        "No" => false,
        _ => return Err("Choose Yes or No for non-negative variables".into()),
    };
    let linear = match v[6].as_str() {
        "Linear (simplex)" => true,
        "Nonlinear (Nelder–Mead)" => false,
        _ => return Err("Choose a Solver method".into()),
    };
    Ok(SolverModel {
        objective,
        goal,
        variables,
        constraints,
        non_negative,
        linear,
        extra: book
            .solver_model()
            .map(|m| m.extra.clone())
            .unwrap_or_default(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Vec<String> {
        [
            "C1",
            "Maximum",
            "0",
            "A1:B1",
            "A2 <= 4\nA3 <= 12\nA4 <= 18",
            "Yes",
            "Linear (simplex)",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    fn book() -> Workbook {
        let mut b = Workbook::new(5, 5).unwrap();
        for (r, c, v) in [
            (0, 0, "0"),
            (0, 1, "0"),
            (0, 2, "=3*A1+5*B1"),
            (1, 0, "=A1"),
            (2, 0, "=2*B1"),
            (3, 0, "=3*A1+2*B1"),
        ] {
            b.set_cell(r, c, v.into()).unwrap();
        }
        b
    }
    fn wait(s: &mut Session, b: &mut Workbook) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !s.poll(b) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    fn native_form_prefill_parser_workers_no_yes_undo_cancel_and_stale_results() {
        let mut b = book();
        let m = parse(&input(), &b).unwrap();
        let mut s = Session::default();
        s.start(b.solver_task(m.clone()).unwrap()).unwrap();
        wait(&mut s, &mut b);
        assert_eq!(b.cell(0, 0), Some("0"));
        assert_eq!(s.take_command(), Some(RESULT));
        assert!(s.take_command().is_none());
        assert!(matches!(
            s.dialog(),
            Some(win42::DialogRequest::Confirmation { .. })
        ));
        s.accept(&mut b, false).unwrap();
        assert_eq!(b.cell(0, 0), Some("0"));
        assert_eq!(b.solver_model(), Some(&m));
        let req = request(&b, (1, 1));
        let saved: Vec<_> = req.fields.into_iter().map(|(_, v)| v).collect();
        assert_eq!(parse(&saved, &b).unwrap(), m);
        s.start(b.solver_task(m.clone()).unwrap()).unwrap();
        wait(&mut s, &mut b);
        s.accept(&mut b, true).unwrap();
        assert_eq!(b.cell(0, 0), Some("2"));
        assert_eq!(b.cell(0, 1), Some("6"));
        b.undo();
        assert_eq!(b.cell(0, 0), Some("0"));
        assert_eq!(b.solver_model(), Some(&m));
        s.start(b.solver_task(m.clone()).unwrap()).unwrap();
        wait(&mut s, &mut b);
        b.set_cell(4, 4, "changed".into()).unwrap();
        assert!(s.accept(&mut b, true).is_err());
        assert_eq!(b.cell(0, 0), Some("0"));
        s.start(b.solver_task(m).unwrap()).unwrap();
        s.cancel();
        assert!(!s.running());
        assert!(!s.poll(&mut b));
        assert!(!s.has_outcome());
        assert!(s.take_command().is_none());
        let mut bad = input();
        bad[3] = "A1:E5".into();
        assert!(parse(&bad, &b).is_ok());
        bad[3] = "A1:Z999".into();
        assert!(parse(&bad, &b).is_err());
        bad = input();
        bad[4] = "A1 <= NaN".into();
        assert!(b.solver_task(parse(&bad, &b).unwrap()).is_err());
        bad[4] = "A1 integer 3".into();
        assert!(parse(&bad, &b).is_err());
    }
    #[test]
    fn infeasible_model_is_kept_but_failed_trials_leave_cell_values_unchanged() {
        let mut b = book();
        let mut v = input();
        v[4] = "A1 <= 0\nA1 >= 1".into();
        let m = parse(&v, &b).unwrap();
        let mut s = Session::default();
        s.start(b.solver_task(m.clone()).unwrap()).unwrap();
        wait(&mut s, &mut b);
        assert!(matches!(
            s.dialog(),
            Some(win42::DialogRequest::Information { .. })
        ));
        assert_eq!(b.solver_model(), Some(&m));
        assert_eq!(b.cell(0, 0), Some("0"));
        b.undo();
        assert!(b.solver_model().is_none());
    }
}
