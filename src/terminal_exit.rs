use super::AppResult;

pub(super) trait ExitApp {
    type Seal;
    type SaveError: std::error::Error + 'static;
    fn begin_exit_work(&mut self) -> Self::Seal;
    fn poll_exit_work(&mut self, seal: &Self::Seal, budget: usize) -> AppResult<bool>;
    fn save_exit_state(&self) -> Result<(), Self::SaveError>;
}

/// Normal terminal exit: receipts first, checked save next, cleanup exactly once.
/// Tests inject cleanup using owned state and never initialize a terminal.
pub(super) fn finish_exit<A: ExitApp>(
    app: &mut A,
    cleanup: impl FnOnce(&mut A) -> AppResult<()>,
) -> AppResult<()> {
    let seal = app.begin_exit_work();
    loop {
        match app.poll_exit_work(&seal, 64) {
            Ok(false) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Ok(true) => break,
            Err(settlement_error) => {
                if let Err(cleanup_error) = cleanup(app) {
                    eprintln!("Terminal cleanup failed: {cleanup_error}");
                }
                return Err(settlement_error);
            }
        }
    }
    let saved = app.save_exit_state();
    let cleaned = cleanup(app);
    match (saved, cleaned) {
        (Err(save_error), Err(cleanup_error)) => {
            eprintln!("Terminal cleanup failed: {cleanup_error}");
            Err(save_error.into())
        }
        (Err(save_error), Ok(())) => Err(save_error.into()),
        (Ok(()), cleaned) => cleaned,
    }
}
