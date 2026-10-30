//! PLAN P2.9: the ALGORITHMS §10 step-1 anchors hold with the committed calibration constants.

#[test]
fn sweep_anchors_hold() {
    let cal = clippro_measure::calibration::Calibration::embedded();
    let r = clippro_core::calibrate::sweep(cal);
    for a in &r.anchors {
        eprintln!("{} {:<45} {:.3} (want {})", if a.pass { "PASS" } else { "FAIL" }, a.name, a.measured, a.want);
    }
    for p in &r.proposals {
        eprintln!("proposal {} {:.4} -> {:.4} ({})", p.key, p.current, p.proposed, p.why);
    }
    assert!(clippro_core::calibrate::report_html(&r, "test").contains("Anchors"));
    assert!(r.passed(), "anchors failed");
}
