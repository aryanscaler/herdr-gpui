use super::*;

#[test]
fn an_uncertain_start_reports_its_terminal_without_claiming_prompt_success() {
    let reports = std::cell::RefCell::new(Vec::new());
    let result = retain_started(
        Err(work::fanout::Error::Work(work::Error::Unprompted {
            started: work::Started::Herdr2 {
                terminal: "t9".into(),
            },
            source: Box::new(work::Error::Cancelled),
        })),
        2,
        &|report| {
            reports.borrow_mut().push(report);
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::Work(_))));
    assert!(
        matches!(reports.borrow().as_slice(), [Report::Lane(2, Progress::Terminal(terminal))] if terminal == "t9")
    );
}
