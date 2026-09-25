#[test]
fn probe_temp_data_across_passes() {
    use std::time::{Duration, Instant};
    #[derive(Debug, Clone, Copy)]
    struct S { start: Instant, last: Instant }
    let ctx = egui::Context::default();
    let id = egui::Id::new("probe");
    ctx.begin_pass(egui::RawInput::default());
    let e1 = ctx.data_mut(|d| {
        let st = d.get_temp::<S>(id);
        d.insert_temp(id, S { start: Instant::now(), last: Instant::now() });
        st.is_none()
    });
    ctx.end_pass();
    ctx.begin_pass(egui::RawInput::default());
    let e2 = ctx.data_mut(|d| d.get_temp::<S>(id).is_none());
    ctx.end_pass();
    ctx.begin_pass(egui::RawInput::default());
    let e3 = ctx.data_mut(|d| d.get_temp::<S>(id).is_none());
    println!("first_call_fresh={e1} second_pass_missing={e2} third_pass_missing={e3}");
    assert!(e1, "first call must be fresh");
    assert!(!e2, "temp data must survive into second pass");
    assert!(!e3, "temp data must survive into third pass");
}
