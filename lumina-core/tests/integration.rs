use lumina_core::{
    BitRegister, BitWidth, ErrorKind, LineOutput, Notebook, Radix, SnapKind, compile_plot,
    find_snap_points, sample_curve,
};

#[test]
fn bilingual_notebook_resilient_to_inline_errors() {
    let doc = "\
# Инженерный расчёт
v = 120 км/ч в м/с
bad = 1 / 0
t = 2 мин в с
s = v * t в км
#5 in mi
sum";

    let nb = Notebook::from_text(doc);
    assert_eq!(nb.lines().len(), 7);
    assert!(matches!(nb.lines()[0].output, LineOutput::Empty));
    assert!(nb.lines()[0].is_heading);

    let v_str = nb.lines()[1].output.value().expect("v").plain(4);
    assert_eq!(v_str, "33.3333 м/с");

    let err = nb.lines()[2].output.error().expect("div0");
    assert_eq!(err.kind, ErrorKind::DivisionByZero);

    let t_str = nb.lines()[3].output.value().expect("t").plain(2);
    assert_eq!(t_str, "120 с");

    let s_str = nb.lines()[4].output.value().expect("s").plain(4);
    assert_eq!(s_str, "4 км");

    let mi_val = nb.lines()[5].output.value().expect("mi").display_magnitude();
    let expected = rust_decimal::Decimal::new(24855, 4);
    assert!((mi_val - expected).abs() < rust_decimal::Decimal::new(1, 3));

    let sum_err = nb.lines()[6].output.error().expect("dim mismatch");
    assert!(matches!(sum_err.kind, ErrorKind::DimensionMismatch(_, _)));
}

#[test]
fn plotter_russian_functions_and_snap_points() {
    let f = compile_plot("y = a * син(b * x)").expect("ru sin");
    let g = compile_plot("y = 0.5 * x").expect("line");
    assert_eq!(f.params.len(), 2);
    assert_eq!(f.params[0].as_ref(), "a");
    assert_eq!(f.params[1].as_ref(), "b");

    let params = vec![(Box::from("a"), 2.0), (Box::from("b"), 1.0)];
    let segs = sample_curve(&f, -6.0, 6.0, 6.0, 400, &params);
    assert_eq!(segs.len(), 1);
    assert!(segs[0].len() > 350);

    let snaps = find_snap_points(&[&f, &g], -4.0, 4.0, -3.0, 3.0, 400, &params);
    assert!(snaps.iter().any(|p| p.kind == SnapKind::Root && p.x.abs() < 1e-3));
    assert!(snaps
        .iter()
        .any(|p| p.kind == SnapKind::Intersection && p.x > 2.0 && p.x < 2.6));
}

#[test]
fn programmer_register_full_roundtrip() {
    let mut reg = BitRegister::new(0, BitWidth::Bits32, false);
    assert!(reg.parse_radix("0xCAFE_BABE", Radix::Hex));
    assert_eq!(reg.format_hex(false), "CAFEBABE");
    reg.rol(8);
    assert_eq!(reg.format_hex(false), "FEBABECA");
    reg.ror(8);
    assert_eq!(reg.format_hex(false), "CAFEBABE");

    reg.set_width(BitWidth::Bits16);
    assert_eq!(reg.format_hex(false), "BABE");
    reg.signed = true;
    assert!(reg.as_signed() < 0);
}
