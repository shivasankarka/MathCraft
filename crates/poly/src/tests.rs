use super::*;

fn x(n: usize, i: usize) -> Poly {
    Poly::var(n, i).unwrap()
}
fn c(n: usize, v: i64) -> Poly {
    Poly::constant(n, IBig::from(v))
}

#[test]
fn monomial_packing_and_divisibility() {
    let m = pack(&[2, 0, 5]).unwrap();
    assert_eq!((exponent(m, 0), exponent(m, 1), exponent(m, 2)), (2, 0, 5));
    assert!(pack(&[128]).is_none() && pack(&[0; 9]).is_none());
    assert!(divides(pack(&[1, 0, 2]).unwrap(), m));
    assert!(!divides(pack(&[3, 0, 0]).unwrap(), m));
    assert!(!divides(pack(&[0, 1, 0]).unwrap(), m));
    assert!(divides(0, m));
}

#[test]
fn add_sub_and_cancellation() {
    // (x + y) + (x - y) = 2x, (x + y) - (x + y) = 0
    let (a, b) = (x(2, 0), x(2, 1));
    let s = a.add(&b).unwrap();
    assert_eq!(s.len(), 2);
    assert_eq!(s.add(&a.sub(&b).unwrap()).unwrap(), a.scale(&IBig::from(2)));
    assert!(s.sub(&s).unwrap().is_zero());
    assert_eq!(s.add(&Poly::zero(2)).unwrap(), s);
    assert_eq!(s.add(&Poly::zero(3)), Err(PolyError::Mismatch));
}

#[test]
fn products_and_powers() {
    let (a, b) = (x(2, 0), x(2, 1));
    // (x + y)^2 = x^2 + 2xy + y^2
    let s = a.add(&b).unwrap();
    let sq = s.mul(&s).unwrap();
    let expect = Poly::from_terms(2, vec![(pack(&[2, 0]).unwrap(), IBig::ONE), (pack(&[1, 1]).unwrap(), IBig::from(2)), (pack(&[0, 2]).unwrap(), IBig::ONE)]);
    assert_eq!(sq, expect);
    assert_eq!(s.pow(2).unwrap(), expect);
    assert_eq!(s.pow(0).unwrap(), c(2, 1));
    // (x + y)(x - y) = x^2 - y^2: the xy terms cancel
    assert_eq!(s.mul(&a.sub(&b).unwrap()).unwrap().len(), 2);
    // binomial coefficients: (x + 1)^10 has 11 terms, the middle one is 252
    let p = a.add(&c(2, 1)).unwrap().pow(10).unwrap();
    assert_eq!(p.len(), 11);
    assert!(p.terms().iter().any(|(_, k)| *k == IBig::from(252)));
    // exponent overflow is reported, not wrapped
    assert_eq!(a.pow(100).unwrap().mul(&a.pow(100).unwrap()), Err(PolyError::TooBig));
}

#[test]
fn product_matches_schoolbook_on_random_polynomials() {
    // a tiny deterministic generator keeps the test dependency-free
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut random = |n: usize, terms: usize| {
        let ts = (0..terms)
            .map(|_| (pack(&[(next() % 5) as u32, (next() % 5) as u32, (next() % 5) as u32]).unwrap(), IBig::from((next() % 21) as i64 - 10)))
            .collect();
        Poly::from_terms(n, ts)
    };
    for _ in 0..30 {
        let (a, b) = (random(3, 12), random(3, 9));
        let mut naive = Vec::new();
        for (ma, ca) in a.terms() {
            for (mb, cb) in b.terms() {
                naive.push((ma + mb, ca * cb));
            }
        }
        assert_eq!(a.mul(&b).unwrap(), Poly::from_terms(3, naive));
    }
}

#[test]
fn heap_and_table_products_agree_with_big_coefficients() {
    // coefficients above i64 force the heap path; the same polynomials scaled down use the table
    let big = IBig::from(10).pow(30);
    let a = benchmark_poly([1, 2, 3, 4, 5, 6, 7], 0).pow(1).unwrap();
    let b = benchmark_poly([1, -1, 1, -1, 1, -1, 1], 2);
    let small = a.mul(&b).unwrap();
    let scaled = a.scale(&big).mul(&b).unwrap();
    assert_eq!(scaled, small.scale(&big));
    assert!(small.len() > SMALL_PRODUCT);
}

#[test]
fn exact_division() {
    let (a, b) = (x(2, 0), x(2, 1));
    let p = a.add(&b).unwrap().pow(3).unwrap();
    let q = a.sub(&b).unwrap();
    let product = p.mul(&q).unwrap();
    assert_eq!(product.div_exact(&q).unwrap(), Some(p.clone()));
    assert_eq!(product.div_exact(&p).unwrap(), Some(q));
    assert_eq!(p.add(&c(2, 1)).unwrap().div_exact(&a.sub(&b).unwrap()).unwrap(), None);
    assert_eq!(p.div_exact(&Poly::zero(2)).unwrap(), None);
    // coefficients must divide too: 2x is not divisible by 4
    assert_eq!(a.scale(&IBig::from(2)).div_exact(&c(2, 4)).unwrap(), None);
}

#[test]
fn degrees() {
    let p = x(3, 0).pow(2).unwrap().mul(&x(3, 2)).unwrap().add(&x(3, 1)).unwrap();
    assert_eq!((p.degree_in(0), p.degree_in(1), p.degree_in(2), p.total_degree()), (2, 1, 1, 3));
}

/// `(1 + c1 x1 + ... + c7 x7)^7 + k` with the given coefficients.
fn benchmark_poly(coeffs: [i64; 7], k: i64) -> Poly {
    let mut p = c(7, 1);
    for (i, k) in coeffs.iter().enumerate() {
        p = p.add(&x(7, i).scale(&IBig::from(*k))).unwrap();
    }
    p.pow(7).unwrap().add(&c(7, k)).unwrap()
}

#[test]
#[ignore = "timing; run with --release --ignored --nocapture"]
fn timing_of_a_large_product() {
    let t = std::time::Instant::now();
    let a = benchmark_poly([3, 5, 7, 9, 11, 13, 15], -1);
    let g = benchmark_poly([3, 5, 7, 9, 11, 13, -15], 3);
    println!("build a, g ({} and {} terms): {:?}", a.len(), g.len(), t.elapsed());
    let t = std::time::Instant::now();
    let ag = a.mul(&g).unwrap();
    println!("a*g: {} terms in {:?}", ag.len(), t.elapsed());
    let b = benchmark_poly([-3, -5, -7, 9, -11, -13, 15], 1);
    let bg = b.mul(&g).unwrap();
    let t = std::time::Instant::now();
    let deadline = t + std::time::Duration::from_secs(120);
    match ag.gcd_with(&bg, &mut || std::time::Instant::now() > deadline) {
        Ok(h) => println!("gcd: {} terms in {:?}; equals g: {}", h.len(), t.elapsed(), h == g),
        Err(e) => println!("gcd gave up after {:?}: {e}", t.elapsed()),
    }
}

#[test]
fn gcd_of_polynomials() {
    let (a, b, z) = (x(3, 0), x(3, 1), x(3, 2));
    let s = a.add(&b).unwrap();
    let d = a.sub(&b).unwrap();
    // gcd((x+y)^3 (x-y), (x+y)^2 (y+z)) = (x+y)^2
    let f = s.pow(3).unwrap().mul(&d).unwrap();
    let g = s.pow(2).unwrap().mul(&b.add(&z).unwrap()).unwrap();
    assert_eq!(f.gcd(&g).unwrap(), s.pow(2).unwrap());
    // content: gcd(6 (x+y), 4 (x+y)(x-y)) = 2 (x+y)
    let h1 = s.scale(&IBig::from(6));
    let h2 = s.mul(&d).unwrap().scale(&IBig::from(4));
    assert_eq!(h1.gcd(&h2).unwrap(), s.scale(&IBig::from(2)));
    // coprime
    assert_eq!(a.gcd(&b).unwrap(), c(3, 1));
    assert_eq!(s.gcd(&d).unwrap(), c(3, 1));
    // constants, zero, sign
    assert_eq!(c(3, 12).gcd(&c(3, -18)).unwrap(), c(3, 6));
    assert_eq!(Poly::zero(3).gcd(&s.neg()).unwrap(), s);
    assert_eq!(f.gcd(&Poly::zero(3)).unwrap(), f);
    // a common factor in only some variables: gcd(x z, y z) = z
    assert_eq!(a.mul(&z).unwrap().gcd(&b.mul(&z).unwrap()).unwrap(), z);
}

#[test]
fn gcd_divides_both_and_cofactors_are_coprime() {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut random = |terms: usize| {
        let ts = (0..terms)
            .map(|_| (pack(&[(next() % 3) as u32, (next() % 3) as u32, (next() % 3) as u32]).unwrap(), IBig::from((next() % 9) as i64 - 4)))
            .collect();
        Poly::from_terms(3, ts)
    };
    for _ in 0..25 {
        let (common, p, q) = (random(4), random(3), random(3));
        if common.is_zero() || p.is_zero() || q.is_zero() {
            continue;
        }
        let (f, g) = (common.mul(&p).unwrap(), common.mul(&q).unwrap());
        let h = f.gcd(&g).unwrap();
        assert!(f.div_exact(&h).unwrap().is_some() && g.div_exact(&h).unwrap().is_some(), "gcd divides both");
        // the common factor divides the gcd
        assert!(h.div_exact(&common).unwrap().is_some() || h.div_exact(&common.neg()).unwrap().is_some(), "{common:?} | {h:?}");
        let (cf, cg) = (f.div_exact(&h).unwrap().unwrap(), g.div_exact(&h).unwrap().unwrap());
        assert_eq!(cf.gcd(&cg).unwrap(), c(3, 1).scale(&cf.gcd(&cg).unwrap().content()), "cofactors are coprime up to an integer");
    }
}

// ---- factoring -----------------------------------------------------------------------------------------------------

/// A univariate polynomial from coefficients, lowest degree first.
fn up(coeffs: &[i64]) -> Poly {
    let terms = coeffs.iter().enumerate().filter(|(_, c)| **c != 0).map(|(i, c)| (pack(&[i as u32]).unwrap(), IBig::from(*c))).collect();
    Poly::from_terms(1, terms)
}

/// Factor, check that the factors multiply back, and return the degrees of the factors with multiplicity.
fn factor_degrees(p: &Poly) -> Vec<u32> {
    let f = p.factor().unwrap().expect("factorisation gave up");
    let mut prod = Poly::constant(p.nvars(), f.content.clone());
    for (g, k) in &f.factors {
        prod = prod.mul(&g.pow(*k).unwrap()).unwrap();
    }
    assert_eq!(&prod, p, "factors do not multiply back");
    let mut degs: Vec<u32> = f.factors.iter().flat_map(|(g, k)| std::iter::repeat_n(g.total_degree(), *k as usize)).collect();
    degs.sort_unstable();
    degs
}

#[test]
fn factors_integer_polynomials_in_one_variable() {
    assert_eq!(factor_degrees(&up(&[4, 0, 0, 0, 1])), vec![2, 2]); // x^4 + 4
    assert_eq!(factor_degrees(&up(&[1, 0, 1, 0, 1])), vec![2, 2]); // x^4 + x^2 + 1
    assert_eq!(factor_degrees(&up(&[-1, 0, 0, 0, 0, 0, 0, 0, 1])), vec![1, 1, 2, 4]); // x^8 - 1
    assert_eq!(factor_degrees(&up(&[-1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])), vec![1, 1, 2, 2, 2, 4]); // x^12 - 1
    assert_eq!(factor_degrees(&up(&[1, 0, -10, 0, 1])), vec![4]); // splits modulo every prime, irreducible over Z
    assert_eq!(factor_degrees(&up(&[-2, 0, 0, 0, 1])), vec![4]); // x^4 - 2
    assert_eq!(factor_degrees(&up(&[1, 0, 1])), vec![2]);
    assert_eq!(factor_degrees(&up(&[-4, 5, 6])), vec![1, 1]); // 6x^2 + 5x - 4
    assert_eq!(factor_degrees(&up(&[7])), Vec::<u32>::new());
}

#[test]
fn repeated_and_large_factors() {
    // (x + 1)^5 (x - 2)^3 (2x + 3)
    let mut p = up(&[2, 3]);
    for _ in 0..5 {
        p = p.mul(&up(&[1, 1])).unwrap();
    }
    for _ in 0..3 {
        p = p.mul(&up(&[-2, 1])).unwrap();
    }
    assert_eq!(factor_degrees(&p), vec![1; 9]);
    // (x - 1)(x - 2)...(x - 9) has nine linear factors that must all be recovered
    let mut w = up(&[1]);
    for r in 1..=9 {
        w = w.mul(&up(&[-r, 1])).unwrap();
    }
    assert_eq!(factor_degrees(&w), vec![1; 9]);
    // a product of two quartics with big coefficients
    let a = up(&[3, -7, 0, 11, 5]);
    let b = up(&[-13, 2, 9, 0, 1]);
    assert_eq!(factor_degrees(&a.mul(&b).unwrap()), vec![4, 4]);
}

#[test]
fn content_and_sign_are_separated() {
    let f = up(&[-6, 0, 6]).factor().unwrap().unwrap(); // 6x^2 - 6 = 6 (x - 1)(x + 1)
    assert_eq!(f.content, IBig::from(6));
    let g = up(&[6, 0, -6]).factor().unwrap().unwrap();
    assert_eq!(g.content, IBig::from(-6));
}

#[test]
fn factors_in_several_variables() {
    let (x0, y0, z0) = (x(3, 0), x(3, 1), x(3, 2));
    // x^2 y - y^3 = y (x - y)(x + y)
    let p = y0.mul(&x0.mul(&x0).unwrap().sub(&y0.mul(&y0).unwrap()).unwrap()).unwrap();
    assert_eq!(factor_degrees(&p), vec![1, 1, 1]);
    // x^3 + y^3 + z^3 - 3xyz = (x + y + z)(x^2 + y^2 + z^2 - xy - yz - zx)
    let cube = |a: &Poly| a.mul(a).unwrap().mul(a).unwrap();
    let three_xyz = x0.mul(&y0).unwrap().mul(&z0).unwrap().scale(&IBig::from(3));
    let p = cube(&x0).add(&cube(&y0)).unwrap().add(&cube(&z0)).unwrap().sub(&three_xyz).unwrap();
    assert_eq!(factor_degrees(&p), vec![1, 2]);
    // x^4 y^2 - y^2 + x^2 y^4 - y^4 = y^2 (x^2 - 1)(x^2 + y^2 ... ) style product
    let (a, b) = (x(2, 0), x(2, 1));
    let one = c(2, 1);
    let q = a.mul(&a).unwrap().sub(&one).unwrap().mul(&a.mul(&a).unwrap().add(&b.mul(&b).unwrap()).unwrap()).unwrap();
    assert_eq!(factor_degrees(&q.mul(&b.mul(&b).unwrap()).unwrap()), vec![1, 1, 1, 1, 2]);
    // irreducible in two variables
    let irr = a.mul(&a).unwrap().add(&b.mul(&b).unwrap()).unwrap().add(&one).unwrap();
    assert_eq!(factor_degrees(&irr), vec![2]);
}

/// A small deterministic random polynomial in `n` variables of total degree at most `deg`.
fn random_poly(n: usize, deg: u32, terms: usize, seed: &mut u64) -> Poly {
    let mut next = move |m: u64| {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed % m
    };
    let mut ts = Vec::new();
    for _ in 0..terms {
        let mut exps = vec![0u32; n];
        let mut left = deg;
        for e in exps.iter_mut() {
            *e = next(u64::from(left) + 1) as u32;
            left -= *e;
        }
        let k = next(19) as i64 - 9;
        if k != 0 {
            ts.push((pack(&exps).unwrap(), IBig::from(k)));
        }
    }
    Poly::from_terms(n, ts)
}

#[test]
fn modular_gcd_matches_euclid_on_random_inputs() {
    let mut seed = 0x1234_5678_9abc_def1u64;
    let mut checked = 0;
    for (n, dg, da) in [(2usize, 3u32, 3u32), (3, 3, 3), (3, 4, 2), (4, 3, 2), (5, 2, 2)] {
        for _ in 0..6 {
            let (g, a, b) = (random_poly(n, dg, 12, &mut seed), random_poly(n, da, 10, &mut seed), random_poly(n, da, 10, &mut seed));
            if g.is_zero() || a.is_zero() || b.is_zero() {
                continue;
            }
            let (f1, f2) = (a.mul(&g).unwrap(), b.mul(&g).unwrap());
            let Some(v) = (0..n).find(|v| f1.degree_in(*v) > 0 && f2.degree_in(*v) > 0) else { continue };
            // both primitive in x_v, as the caller guarantees
            let prim = |p: &Poly| {
                let c = p.coeffs_in(v).iter().fold(Poly::zero(n), |c, k| c.gcd(k).unwrap());
                p.div_exact(&c).unwrap().unwrap()
            };
            let (p1, p2) = (prim(&f1), prim(&f2));
            let want = p1.gcd(&p2).unwrap();
            let got = modgcd::gcd_primitive(&p1, &p2, v, 0, &mut || false).unwrap();
            if let Some(got) = got {
                assert_eq!(got, want, "n={n}");
                checked += 1;
            }
        }
    }
    assert!(checked >= 10, "the modular method should apply to most of these, applied to {checked}");
}

#[test]
fn modular_gcd_combines_primes_for_large_coefficients() {
    // the leading coefficient 10^27 does not fit one 62-bit prime
    let (x, y, z) = (x(3, 0), x(3, 1), x(3, 2));
    let big = IBig::from(10).pow(9);
    let g = x.scale(&big).add(&y).unwrap().add(&z).unwrap().add(&c(3, 1)).unwrap().pow(3).unwrap();
    let f1 = g.mul(&x.add(&y).unwrap()).unwrap();
    let f2 = g.mul(&x.sub(&z).unwrap().add(&c(3, 2)).unwrap()).unwrap();
    let got = modgcd::gcd_primitive(&f1, &f2, 0, 0, &mut || false).unwrap();
    assert_eq!(got, Some(g.clone()));
    assert_eq!(f1.gcd(&f2).unwrap(), g);
}

#[test]
fn modular_gcd_of_coprime_inputs_is_one() {
    let (x, y, z) = (x(3, 0), x(3, 1), x(3, 2));
    let f = x.pow(3).unwrap().add(&y.mul(&z).unwrap()).unwrap().add(&c(3, 1)).unwrap();
    let g = x.pow(2).unwrap().add(&y.pow(2).unwrap()).unwrap().add(&z).unwrap();
    assert_eq!(modgcd::gcd_primitive(&f, &g, 0, 0, &mut || false).unwrap(), Some(c(3, 1)));
}
