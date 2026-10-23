//! Nelder-Mead simplex, used to fit a modern model's parameters to a Kontakt reference curve.

/// Minimise `f` from `x0`; `step` sets the initial simplex size per axis. Returns the best point.
pub fn nelder_mead(f: impl Fn(&[f64]) -> f64, x0: &[f64], step: &[f64], iters: usize) -> Vec<f64> {
    let n = x0.len();
    let mut simplex: Vec<Vec<f64>> = vec![x0.to_vec()];
    for i in 0..n {
        let mut p = x0.to_vec();
        p[i] += step[i];
        simplex.push(p);
    }
    let mut values: Vec<f64> = simplex.iter().map(|p| f(p)).collect();
    for _ in 0..iters {
        let mut order: Vec<usize> = (0..=n).collect();
        order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
        simplex = order.iter().map(|&i| simplex[i].clone()).collect();
        values = order.iter().map(|&i| values[i]).collect();
        let spread = (values[n] - values[0]).abs();
        if spread < 1e-12 {
            break;
        }
        let centroid: Vec<f64> = (0..n).map(|d| simplex[..n].iter().map(|p| p[d]).sum::<f64>() / n as f64).collect();
        let along = |t: f64| -> Vec<f64> { (0..n).map(|d| centroid[d] + t * (simplex[n][d] - centroid[d])).collect() };
        let reflected = along(-1.0);
        let fr = f(&reflected);
        if fr < values[0] {
            let expanded = along(-2.0);
            let fe = f(&expanded);
            if fe < fr {
                simplex[n] = expanded;
                values[n] = fe;
            } else {
                simplex[n] = reflected;
                values[n] = fr;
            }
        } else if fr < values[n - 1] {
            simplex[n] = reflected;
            values[n] = fr;
        } else {
            let contracted = along(if fr < values[n] { -0.5 } else { 0.5 });
            let fc = f(&contracted);
            if fc < values[n].min(fr) {
                simplex[n] = contracted;
                values[n] = fc;
            } else {
                for i in 1..=n {
                    simplex[i] = (0..n).map(|d| simplex[0][d] + 0.5 * (simplex[i][d] - simplex[0][d])).collect();
                    values[i] = f(&simplex[i]);
                }
            }
        }
    }
    let best = (0..=n).min_by(|&a, &b| values[a].total_cmp(&values[b])).unwrap_or(0);
    simplex[best].clone()
}

#[cfg(test)]
mod tests {
    use super::nelder_mead;

    #[test]
    fn finds_the_minimum_of_a_quadratic_bowl() {
        let best = nelder_mead(|p| (p[0] - 1.0).powi(2) + (p[1] + 2.0).powi(2), &[0.0, 0.0], &[0.5, 0.5], 200);
        assert!((best[0] - 1.0).abs() < 1e-4 && (best[1] + 2.0).abs() < 1e-4, "{best:?}");
    }
}
