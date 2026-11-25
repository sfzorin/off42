use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use xls42_core::{GoalSeekResult, GoalSeekTask, Workbook};
pub const START: u16 = 79;
pub const CANCEL: u16 = 80;
pub const RESULT: u16 = 81;
struct Worker {
    rx: mpsc::Receiver<Result<GoalSeekResult, String>>,
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
    outcome: Option<Result<GoalSeekResult, String>>,
    ready: bool,
}
impl Session {
    pub fn running(&self) -> bool {
        self.worker.is_some()
    }
    pub fn has_outcome(&self) -> bool {
        self.outcome.is_some()
    }
    pub fn start(&mut self, task: GoalSeekTask) -> Result<(), String> {
        self.cancel();
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("off42-goal-seek".into())
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
    pub fn poll(&mut self) -> bool {
        let result = match self.worker.as_ref().map(|worker| worker.rx.try_recv()) {
            Some(Ok(result)) => Some(result),
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                Some(Err("Goal Seek stopped unexpectedly".into()))
            }
            _ => None,
        };
        if let Some(result) = result {
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
            Ok(result) => win42::DialogRequest::Confirmation {
                title: "Goal Seek found a solution",
                text: {
                    let question = win42::localization::text("%@ = %@. Keep the result?")
                        .replacen("%@", &reference(result.variable()), 1)
                        .replacen("%@", &result.value().to_string(), 1);
                    format!(
                        "{} = {}\n{} {}\n\n{}",
                        reference(result.formula()),
                        result.calculated(),
                        win42::localization::text("Target value:"),
                        result.target(),
                        question
                    )
                },
            },
            Err(error) => win42::DialogRequest::Information {
                title: "Goal Seek",
                text: error.clone(),
            },
        })
    }
    pub fn accept(&mut self, book: &mut Workbook, keep: bool) -> Result<(), String> {
        let result = self.outcome.take();
        self.ready = false;
        if keep {
            let result = result.ok_or("Goal Seek result is unavailable")??;
            book.apply_goal_seek(&result)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}
pub fn reference((row, col): (usize, usize)) -> String {
    let mut n = col + 1;
    let mut letters = Vec::new();
    while n > 0 {
        n -= 1;
        letters.push((b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    format!(
        "{}{}",
        letters.into_iter().rev().collect::<String>(),
        row + 1
    )
}
pub fn parse_reference(value: &str, book: &Workbook) -> Result<(usize, usize), String> {
    let value = value.trim();
    if value.len() > 20 {
        return Err("Enter a cell address such as A1".into());
    }
    let value = value.replace('$', "");
    let split = value
        .find(|c: char| !c.is_ascii_alphabetic())
        .ok_or("Enter a cell address such as A1")?;
    let (column, row) = value.split_at(split);
    if column.is_empty() || row.is_empty() || !row.chars().all(|c| c.is_ascii_digit()) {
        return Err("Enter a cell address such as A1".into());
    }
    let column = column
        .bytes()
        .try_fold(0usize, |n, c| {
            n.checked_mul(26)?
                .checked_add((c.to_ascii_uppercase() - b'A' + 1) as usize)
        })
        .and_then(|n| n.checked_sub(1))
        .ok_or("Invalid cell address")?;
    let row = row
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .ok_or("Invalid cell address")?;
    if row >= book.rows() || column >= book.cols() {
        return Err("Cell address is outside the current sheet".into());
    }
    Ok((row, column))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn book() -> Workbook {
        let mut book = Workbook::new(2, 4).unwrap();
        book.set_cell(0, 0, "1".into()).unwrap();
        book.set_cell(0, 1, "=A1^2".into()).unwrap();
        book
    }
    fn wait(session: &mut Session) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !session.poll() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    fn cancel_discard_accept_and_stale_background_results_preserve_live_cells() {
        let mut book = book();
        let mut session = Session::default();
        session
            .start(book.goal_seek_task((0, 1), 2., (0, 0)).unwrap())
            .unwrap();
        wait(&mut session);
        assert_eq!(book.cell(0, 0), Some("1"));
        assert_eq!(session.take_command(), Some(RESULT));
        assert!(session.take_command().is_none());
        assert!(matches!(
            session.dialog(),
            Some(win42::DialogRequest::Confirmation { .. })
        ));
        session.accept(&mut book, false).unwrap();
        assert_eq!(book.cell(0, 0), Some("1"));
        session
            .start(book.goal_seek_task((0, 1), 2., (0, 0)).unwrap())
            .unwrap();
        wait(&mut session);
        session.accept(&mut book, true).unwrap();
        assert!((book.cell(0, 0).unwrap().parse::<f64>().unwrap() - 2f64.sqrt()).abs() < 1e-8);
        book.undo();
        assert_eq!(book.cell(0, 0), Some("1"));
        session
            .start(book.goal_seek_task((0, 1), 2., (0, 0)).unwrap())
            .unwrap();
        wait(&mut session);
        book.set_cell(1, 0, "changed".into()).unwrap();
        assert!(session.accept(&mut book, true).is_err());
        assert_eq!(book.cell(0, 0), Some("1"));
        session
            .start(book.goal_seek_task((0, 1), 2., (0, 0)).unwrap())
            .unwrap();
        session.cancel();
        assert!(!session.running());
        assert!(!session.poll());
        assert!(session.take_command().is_none());
    }
    #[test]
    fn references_use_actual_sheet_coordinates() {
        let book = book();
        assert_eq!(parse_reference(" $b$1 ", &book).unwrap(), (0, 1));
        for bad in [
            "A0",
            "A9999999999999999999999999",
            "A1:B1",
            "Other!A1",
            "A1.0",
            "E1",
            "1",
            "A",
        ] {
            assert!(parse_reference(bad, &book).is_err(), "{bad}");
        }
        assert_eq!(reference((99, 27)), "AB100");
    }
}
