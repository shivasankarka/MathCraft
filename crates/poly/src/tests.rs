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
}
