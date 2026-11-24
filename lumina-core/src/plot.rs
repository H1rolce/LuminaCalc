use crate::error::{CalcError, CalcResult, ErrorKind};
use crate::eval;
use crate::lexer::{Tok, lex};
use crate::parser::{BinOp, Expr, ExprKind, PostOp, Target, UnOp, parse_expr};
use crate::units;
use rust_decimal::prelude::ToPrimitive;

#[derive(Debug, Clone)]
pub struct CompiledPlot {
    pub expr: Expr,
    pub indep: Box<str>,
    pub params: Vec<Box<str>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    Root,
    Intersection,
    YIntercept,
    Extremum,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapPoint {
    pub kind: SnapKind,
    pub x: f64,
    pub y: f64,
    pub curve_a: usize,
    pub curve_b: Option<usize>,
}

fn is_angle_unit(name: &str) -> bool {
    units::lookup(name).is_some_and(|u| u.dim.is_none())
}

pub fn compile_plot(src: &str) -> CalcResult<CompiledPlot> {
    let raw_toks = lex(src);
    let toks: Vec<_> = raw_toks.into_iter().filter(|t| t.tok != Tok::Comment).collect();
    if toks.is_empty() {
        return Err(CalcError::new(ErrorKind::UnexpectedEnd, 0..src.len()));
    }

    let mut skip = 0usize;
    let mut indep: Box<str> = "x".into();

    if toks.len() >= 5
        && matches!(toks[0].tok, Tok::Ident(_))
        && toks[1].tok == Tok::LParen
        && matches!(toks[2].tok, Tok::Ident(_))
        && toks[3].tok == Tok::RParen
        && toks[4].tok == Tok::Assign
    {
        if let Tok::Ident(var) = toks[2].tok {
            indep = var.into();
        }
        skip = 5;
    } else if toks.len() >= 2
        && matches!(toks[0].tok, Tok::Ident(_))
        && toks[1].tok == Tok::Assign
    {
        skip = 2;
    }

    let expr = parse_expr(&toks[skip..], src.len())?;
    let mut params = Vec::new();
    collect_params(&expr, &indep, &mut params);
    Ok(CompiledPlot { expr, indep, params })
}

fn push_unique(out: &mut Vec<Box<str>>, name: &str) {
    if !out.iter().any(|p| p.as_ref() == name) {
        out.push(name.into());
    }
}

fn collect_params(expr: &Expr, indep: &str, out: &mut Vec<Box<str>>) {
    match &expr.kind {
        ExprKind::Number(_, _) | ExprKind::LineRef(_) => {}
        ExprKind::Ident(name) => {
            if name.as_ref() != indep
                && eval::constant(name).is_none()
                && eval::function(name).is_none()
                && !is_angle_unit(name)
            {
                push_unique(out, name);
            }
        }
        ExprKind::Unary(_, a) | ExprKind::Postfix(_, a) => collect_params(a, indep, out),
        ExprKind::Binary(_, a, b) => {
            collect_params(a, indep, out);
            collect_params(b, indep, out);
        }
        ExprKind::Call { name, args, .. } => {
            if eval::function(name).is_none() && args.len() == 1 && name.as_ref() != indep {
                push_unique(out, name);
            }
            for a in args {
                collect_params(a, indep, out);
            }
        }
        ExprKind::Convert(a, target) => {
            collect_params(a, indep, out);
            if let Target::Unit(u) = target {
                collect_params(u, indep, out);
            }
        }
    }
}

#[inline]
fn lookup_param(name: &str, params: &[(Box<str>, f64)]) -> Option<f64> {
    params.iter().find(|(k, _)| k.as_ref() == name).map(|(_, v)| *v)
}

#[inline]
fn const_f64(name: &str) -> Option<f64> {
    if let Some(c) = eval::constant(name) {
        return c.to_f64();
    }
    units::lookup(name)
        .filter(|u| u.dim.is_none())
        .and_then(|u| u.factor.to_f64())
}

pub fn eval_f64(expr: &Expr, indep: &str, x: f64, params: &[(Box<str>, f64)]) -> f64 {
    match &expr.kind {
        ExprKind::Number(d, _) => d.to_f64().unwrap_or(f64::NAN),
        ExprKind::LineRef(_) => f64::NAN,
        ExprKind::Ident(name) => {
            if name.as_ref() == indep {
                x
            } else if let Some(v) = lookup_param(name, params) {
                v
            } else {
                const_f64(name).unwrap_or(f64::NAN)
            }
        }
        ExprKind::Unary(op, a) => {
            let v = eval_f64(a, indep, x, params);
            match op {
                UnOp::Pos => v,
                UnOp::Neg => -v,
                UnOp::Not => !(v as i64) as f64,
            }
        }
        ExprKind::Postfix(op, a) => {
            let v = eval_f64(a, indep, x, params);
            match op {
                PostOp::Percent => v * 0.01,
                PostOp::Factorial => {
                    if v < 0.0 || v > 170.0 || v.fract() != 0.0 {
                        f64::NAN
                    } else {
                        let mut acc = 1.0f64;
                        let n = v as u32;
                        for k in 2..=n {
                            acc *= f64::from(k);
                        }
                        acc
                    }
                }
            }
        }
        ExprKind::Binary(op, a, b) => {
            let l = eval_f64(a, indep, x, params);
            let r = eval_f64(b, indep, x, params);
            match op {
                BinOp::Add => l + r,
                BinOp::Sub => l - r,
                BinOp::Mul | BinOp::ImplicitMul => l * r,
                BinOp::Div => l / r,
                BinOp::Mod => l % r,
                BinOp::Pow => {
                    if r.fract() == 0.0 && r.abs() <= 16.0 {
                        l.powi(r as i32)
                    } else {
                        l.powf(r)
                    }
                }
                BinOp::And => ((l as i64) & (r as i64)) as f64,
                BinOp::Or => ((l as i64) | (r as i64)) as f64,
                BinOp::Xor => ((l as i64) ^ (r as i64)) as f64,
                BinOp::Shl => ((l as i64).wrapping_shl(r as u32)) as f64,
                BinOp::Shr => ((l as i64).wrapping_shr(r as u32)) as f64,
            }
        }
        ExprKind::Call { name, args, .. } => {
            let Some(canon) = eval::function(name) else {
                if args.len() == 1 {
                    let lhs = if name.as_ref() == indep {
                        x
                    } else if let Some(v) = lookup_param(name, params) {
                        v
                    } else if let Some(c) = const_f64(name) {
                        c
                    } else {
                        return f64::NAN;
                    };
                    return lhs * eval_f64(&args[0], indep, x, params);
                }
                return f64::NAN;
            };
            let a0 = || args.first().map_or(f64::NAN, |e| eval_f64(e, indep, x, params));
            let a1 = || args.get(1).map_or(f64::NAN, |e| eval_f64(e, indep, x, params));
            match canon {
                "sqrt" => a0().sqrt(),
                "cbrt" => a0().cbrt(),
                "abs" => a0().abs(),
                "round" => {
                    if args.len() == 2 {
                        let p = 10f64.powi(a1() as i32);
                        (a0() * p).round() / p
                    } else {
                        a0().round()
                    }
                }
                "floor" => a0().floor(),
                "ceil" => a0().ceil(),
                "trunc" => a0().trunc(),
                "sign" => {
                    let v = a0();
                    if v.is_nan() {
                        f64::NAN
                    } else if v > 0.0 {
                        1.0
                    } else if v < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                }
                "sin" => a0().sin(),
                "cos" => a0().cos(),
                "tan" => a0().tan(),
                "cot" => 1.0 / a0().tan(),
                "asin" => a0().asin(),
                "acos" => a0().acos(),
                "atan" => a0().atan(),
                "atan2" => a0().atan2(a1()),
                "sinh" => a0().sinh(),
                "cosh" => a0().cosh(),
                "tanh" => a0().tanh(),
                "asinh" => a0().asinh(),
                "acosh" => a0().acosh(),
                "atanh" => a0().atanh(),
                "ln" => a0().ln(),
                "log2" => a0().log2(),
                "log10" => a0().log10(),
                "log" => {
                    if args.len() == 2 {
                        a0().ln() / a1().ln()
                    } else {
                        a0().log10()
                    }
                }
                "exp" => a0().exp(),
                "hypot" => a0().hypot(a1()),
                "min" => args
                    .iter()
                    .map(|e| eval_f64(e, indep, x, params))
                    .fold(f64::INFINITY, f64::min),
                "max" => args
                    .iter()
                    .map(|e| eval_f64(e, indep, x, params))
                    .fold(f64::NEG_INFINITY, f64::max),
                _ => f64::NAN,
            }
        }
        ExprKind::Convert(a, _) => eval_f64(a, indep, x, params),
    }
}

pub fn sample_curve(
    plot: &CompiledPlot,
    x_min: f64,
    x_max: f64,
    y_span: f64,
    samples: usize,
    params: &[(Box<str>, f64)],
) -> Vec<Vec<[f64; 2]>> {
    if !(x_min.is_finite() && x_max.is_finite()) || x_max <= x_min || samples < 2 {
        return Vec::new();
    }
    let y_limit = (y_span.abs().max(1.0)) * 50.0;
    let jump_limit = (y_span.abs().max(1.0)) * 6.0;
    let dx = (x_max - x_min) / (samples - 1) as f64;

    let mut segments: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::with_capacity(samples);
    let mut prev: Option<[f64; 2]> = None;

    for i in 0..samples {
        let x = x_min + dx * i as f64;
        let y = eval_f64(&plot.expr, &plot.indep, x, params);
        if !y.is_finite() || y.abs() > y_limit {
            if cur.len() >= 2 {
                segments.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
            prev = None;
            continue;
        }
        if let Some([px, py]) = prev {
            let dy = (y - py).abs();
            if dy > jump_limit {
                let mid = eval_f64(&plot.expr, &plot.indep, 0.5 * (px + x), params);
                let continuous = mid.is_finite()
                    && ((mid >= py.min(y) - jump_limit * 0.25)
                        && (mid <= py.max(y) + jump_limit * 0.25));
                if !continuous || (py * y < 0.0 && dy > jump_limit * 1.5) {
                    if cur.len() >= 2 {
                        segments.push(std::mem::take(&mut cur));
                    } else {
                        cur.clear();
                    }
                }
            }
        }
        cur.push([x, y]);
        prev = Some([x, y]);
    }
    if cur.len() >= 2 {
        segments.push(cur);
    }
    segments
}

fn bisect(f: impl Fn(f64) -> f64, mut a: f64, mut b: f64, mut fa: f64, iters: usize) -> Option<f64> {
    for _ in 0..iters {
        let m = 0.5 * (a + b);
        let fm = f(m);
        if !fm.is_finite() {
            return None;
        }
        if fm == 0.0 {
            return Some(m);
        }
        if fa.signum() == fm.signum() {
            a = m;
            fa = fm;
        } else {
            b = m;
        }
    }
    Some(0.5 * (a + b))
}

fn golden_min(f: impl Fn(f64) -> f64, mut a: f64, mut b: f64, iters: usize) -> f64 {
    const INV_PHI: f64 = 0.618_033_988_749_894_9;
    let mut c = b - INV_PHI * (b - a);
    let mut d = a + INV_PHI * (b - a);
    let mut fc = f(c);
    let mut fd = f(d);
    for _ in 0..iters {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - INV_PHI * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + INV_PHI * (b - a);
            fd = f(d);
        }
    }
    0.5 * (a + b)
}

fn push_snap_dedup(out: &mut Vec<SnapPoint>, pt: SnapPoint, eps_x: f64, eps_y: f64) {
    if !pt.x.is_finite() || !pt.y.is_finite() {
        return;
    }
    let dup = out.iter().any(|p| {
        p.kind == pt.kind && (p.x - pt.x).abs() <= eps_x && (p.y - pt.y).abs() <= eps_y
    });
    if !dup {
        out.push(pt);
    }
}

pub fn find_snap_points(
    curves: &[&CompiledPlot],
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    steps: usize,
    params: &[(Box<str>, f64)],
) -> Vec<SnapPoint> {
    if !(x_min.is_finite() && x_max.is_finite() && y_min.is_finite() && y_max.is_finite())
        || x_max <= x_min
        || y_max <= y_min
        || steps < 8
    {
        return Vec::new();
    }

    let x_span = x_max - x_min;
    let y_span = y_max - y_min;
    let dx = x_span / steps as f64;
    let eps_x = x_span * 1e-3;
    let eps_y = y_span * 1e-3;
    let root_tol = y_span * 1e-4 + 1e-9;
    let jump_limit = y_span * 4.0;

    let mut out = Vec::new();

    let mut grids: Vec<Vec<f64>> = Vec::with_capacity(curves.len());
    for c in curves {
        let mut ys = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let x = x_min + dx * i as f64;
            ys.push(eval_f64(&c.expr, &c.indep, x, params));
        }
        grids.push(ys);
    }

    for (ci, c) in curves.iter().enumerate() {
        let f = |x: f64| eval_f64(&c.expr, &c.indep, x, params);
        let ys = &grids[ci];

        if x_min <= 0.0 && 0.0 <= x_max {
            let y0 = f(0.0);
            if y0.is_finite() && y0 >= y_min && y0 <= y_max {
                let kind = if y0.abs() < root_tol { SnapKind::Root } else { SnapKind::YIntercept };
                let y = if kind == SnapKind::Root { 0.0 } else { y0 };
                push_snap_dedup(&mut out, SnapPoint { kind, x: 0.0, y, curve_a: ci, curve_b: None }, eps_x, eps_y);
            }
        }

        for i in 0..steps {
            let (y0, y1) = (ys[i], ys[i + 1]);
            if !(y0.is_finite() && y1.is_finite()) || (y1 - y0).abs() > jump_limit {
                continue;
            }
            let x0 = x_min + dx * i as f64;
            let x1 = x0 + dx;
            if y0 == 0.0 {
                push_snap_dedup(
                    &mut out,
                    SnapPoint { kind: SnapKind::Root, x: x0, y: 0.0, curve_a: ci, curve_b: None },
                    eps_x,
                    eps_y,
                );
            } else if y0 * y1 < 0.0
                && let Some(rx) = bisect(f, x0, x1, y0, 40)
                && f(rx).abs() < root_tol
            {
                push_snap_dedup(
                    &mut out,
                    SnapPoint { kind: SnapKind::Root, x: rx, y: 0.0, curve_a: ci, curve_b: None },
                    eps_x,
                    eps_y,
                );
            }
        }

        for i in 1..steps {
            let (y0, y1, y2) = (ys[i - 1], ys[i], ys[i + 1]);
            if !(y0.is_finite() && y1.is_finite() && y2.is_finite()) {
                continue;
            }
            if (y1 - y0).abs() > jump_limit || (y2 - y1).abs() > jump_limit {
                continue;
            }
            let x_l = x_min + dx * (i - 1) as f64;
            let x_r = x_min + dx * (i + 1) as f64;

            let is_min = y1 <= y0 && y1 <= y2 && (y1 < y0 || y1 < y2);
            let is_max = y1 >= y0 && y1 >= y2 && (y1 > y0 || y1 > y2);
            if !is_min && !is_max {
                continue;
            }
            let ex = if is_min {
                golden_min(f, x_l, x_r, 28)
            } else {
                golden_min(|x| -f(x), x_l, x_r, 28)
            };
            let ey = f(ex);
            if !ey.is_finite() || ey < y_min || ey > y_max {
                continue;
            }
            if ey.abs() < root_tol {
                push_snap_dedup(
                    &mut out,
                    SnapPoint { kind: SnapKind::Root, x: ex, y: 0.0, curve_a: ci, curve_b: None },
                    eps_x,
                    eps_y,
                );
            } else {
                push_snap_dedup(
                    &mut out,
                    SnapPoint { kind: SnapKind::Extremum, x: ex, y: ey, curve_a: ci, curve_b: None },
                    eps_x,
                    eps_y,
                );
            }
        }
    }

    for i in 0..curves.len() {
        for j in (i + 1)..curves.len() {
            let (ci, cj) = (curves[i], curves[j]);
            let diff = |x: f64| {
                eval_f64(&ci.expr, &ci.indep, x, params) - eval_f64(&cj.expr, &cj.indep, x, params)
            };
            for k in 0..steps {
                let (a0, a1) = (grids[i][k], grids[i][k + 1]);
                let (b0, b1) = (grids[j][k], grids[j][k + 1]);
                if !(a0.is_finite() && a1.is_finite() && b0.is_finite() && b1.is_finite()) {
                    continue;
                }
                if (a1 - a0).abs() > jump_limit || (b1 - b0).abs() > jump_limit {
                    continue;
                }
                let d0 = a0 - b0;
                let d1 = a1 - b1;
                let x0 = x_min + dx * k as f64;
                let x1 = x0 + dx;
                let candidate = if d0 == 0.0 {
                    Some(x0)
                } else if d0 * d1 < 0.0 {
                    bisect(diff, x0, x1, d0, 40)
                } else {
                    None
                };
                if let Some(ix) = candidate {
                    let ya = eval_f64(&ci.expr, &ci.indep, ix, params);
                    let yb = eval_f64(&cj.expr, &cj.indep, ix, params);
                    if ya.is_finite()
                        && yb.is_finite()
                        && (ya - yb).abs() < root_tol * 2.0
                        && ya >= y_min
                        && ya <= y_max
                    {
                        push_snap_dedup(
                            &mut out,
                            SnapPoint {
                                kind: SnapKind::Intersection,
                                x: ix,
                                y: 0.5 * (ya + yb),
                                curve_a: i,
                                curve_b: Some(j),
                            },
                            eps_x,
                            eps_y,
                        );
                    }
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_free_parameters() {
        let c = compile_plot("y = a * sin(b * x) + c").expect("compile");
        let names: Vec<_> = c.params.iter().map(|s| s.as_ref()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        assert_eq!(c.indep.as_ref(), "x");

        let f = compile_plot("f(t) = A * cos(w * t + pi / 2)").expect("compile");
        let names: Vec<_> = f.params.iter().map(|s| s.as_ref()).collect();
        assert_eq!(names, vec!["A", "w"]);
        assert_eq!(f.indep.as_ref(), "t");
    }

    #[test]
    fn finds_roots_extrema_and_intersections() {
        let p1 = compile_plot("y = x^2 - 4").expect("p1");
        let p2 = compile_plot("y = x - 2").expect("p2");
        let pts = find_snap_points(&[&p1, &p2], -5.0, 5.0, -6.0, 10.0, 400, &[]);

        assert!(pts.iter().any(|p| p.kind == SnapKind::Root && (p.x + 2.0).abs() < 1e-4));
        assert!(pts.iter().any(|p| p.kind == SnapKind::Root && (p.x - 2.0).abs() < 1e-4));
        assert!(pts
            .iter()
            .any(|p| p.kind == SnapKind::Extremum && p.x.abs() < 1e-3 && (p.y + 4.0).abs() < 1e-3));
        assert!(pts
            .iter()
            .any(|p| p.kind == SnapKind::Intersection && (p.x + 1.0).abs() < 1e-4 && (p.y + 3.0).abs() < 1e-4));
        assert!(pts
            .iter()
            .any(|p| p.kind == SnapKind::Intersection && (p.x - 2.0).abs() < 1e-4 && p.y.abs() < 1e-4));
    }

    #[test]
    fn finds_tangential_root() {
        let p = compile_plot("y = (x - 1.5)^2").expect("p");
        let pts = find_snap_points(&[&p], -2.0, 4.0, -1.0, 5.0, 400, &[]);
        assert!(pts
            .iter()
            .any(|pt| pt.kind == SnapKind::Root && (pt.x - 1.5).abs() < 1e-3 && pt.y.abs() < 1e-4));
    }

    #[test]
    fn asymptote_splits_curve_without_fake_root() {
        let p = compile_plot("1 / x").expect("1/x");
        let segs = sample_curve(&p, -5.0, 5.0, 10.0, 400, &[]);
        assert!(segs.len() >= 2, "expected 2 branches for 1/x, got {}", segs.len());
        let snaps = find_snap_points(&[&p], -5.0, 5.0, -10.0, 10.0, 400, &[]);
        assert!(
            !snaps.iter().any(|s| s.kind == SnapKind::Root),
            "1/x must not report a fake root at 0: {snaps:?}"
        );
    }

    #[test]
    fn evaluates_with_parameters() {
        let p = compile_plot("y = a * sin(b * x)").expect("p");
        let params = vec![(Box::from("a"), 3.0), (Box::from("b"), 2.0)];
        let y = eval_f64(&p.expr, &p.indep, std::f64::consts::PI / 4.0, &params);
        assert!((y - 3.0).abs() < 1e-12);
    }

    #[test]
    fn inverse_hyperbolic_sign_nan_and_angle_aliases() {
        let p_asinh = compile_plot("asinh(x)").expect("asinh");
        assert!((eval_f64(&p_asinh.expr, &p_asinh.indep, 1.0, &[]) - 1.0_f64.asinh()).abs() < 1e-12);

        let p_sign = compile_plot("sign(sqrt(x))").expect("sign");
        assert!(eval_f64(&p_sign.expr, &p_sign.indep, -1.0, &[]).is_nan());

        let p_deg = compile_plot("sin(x градус)").expect("deg alias");
        assert!(p_deg.params.is_empty(), "angle unit alias must not become a parameter");
        assert!((eval_f64(&p_deg.expr, &p_deg.indep, 30.0, &[]) - 0.5).abs() < 1e-12);
    }
}
