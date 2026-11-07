//! Integer number theory on machine integers: primality (Miller-Rabin), factorisation (trial division, then
//! Pollard's rho) and the functions built on it.

use mathcraft_core::budget;

fn mul_mod(a: u64, b: u64, m: u64) -> u64 {
    u64::try_from(u128::from(a) * u128::from(b) % u128::from(m)).unwrap_or(0)
}

fn pow_mod(mut b: u64, mut e: u64, m: u64) -> u64 {
    let mut r = 1 % m;
    b %= m;
    while e > 0 {
        if e & 1 == 1 {
            r = mul_mod(r, b, m);
        }
        b = mul_mod(b, b, m);
        e >>= 1;
    }
    r
}

/// Deterministic Miller-Rabin for all `u64` (the first twelve primes as bases are enough below 3.3e24).
pub fn is_prime(n: u64) -> bool {
    const BASES: [u64; 12] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    if n < 2 {
        return false;
    }
    for p in BASES {
        if n.is_multiple_of(p) {
            return n == p;
        }
    }
    let (mut d, mut s) = (n - 1, 0);
    while d.is_multiple_of(2) {
        d /= 2;
        s += 1;
    }
    'bases: for a in BASES {
        let mut x = pow_mod(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 1..s {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                continue 'bases;
            }
        }
        return false;
    }
    true
}

fn gcd(a: u64, b: u64) -> u64 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// A non-trivial factor of the composite `n` (Pollard's rho with Floyd cycle detection).
fn rho(n: u64) -> Option<u64> {
    if n.is_multiple_of(2) {
        return Some(2);
    }
    for c in 1..64u64 {
        let f = |x: u64| (mul_mod(x, x, n) + c) % n;
        let (mut x, mut y, mut d) = (2u64, 2u64, 1u64);
        let mut steps = 0u64;
        while d == 1 {
            steps += 1;
            if steps.is_multiple_of(1024) && budget::tick().is_err() {
                return None;
            }
            x = f(x);
            y = f(f(y));
            d = gcd(x.abs_diff(y), n);
        }
        if d != n {
            return Some(d);
        }
    }
    None
}

/// Prime factors with multiplicity, ascending. `None` when the work budget runs out.
pub fn factor(mut n: u64) -> Option<Vec<(u64, u32)>> {
    let mut out: Vec<(u64, u32)> = Vec::new();
    let push = |p: u64, out: &mut Vec<(u64, u32)>| match out.iter_mut().find(|(q, _)| *q == p) {
        Some((_, k)) => *k += 1,
        None => out.push((p, 1)),
    };
    let mut p = 2u64;
    while p < 1000 && p * p <= n {
        while n.is_multiple_of(p) {
            push(p, &mut out);
            n /= p;
        }
        p += if p == 2 { 1 } else { 2 };
    }
    let mut stack = if n > 1 { vec![n] } else { Vec::new() };
    while let Some(m) = stack.pop() {
        if is_prime(m) {
            push(m, &mut out);
        } else {
            let d = rho(m)?;
            stack.push(d);
            stack.push(m / d);
        }
    }
    out.sort_unstable();
    Some(out)
}

/// Euler's totient from the factorisation.
pub fn totient(n: u64) -> Option<u64> {
    if n == 0 {
        return Some(0);
    }
    Some(factor(n)?.iter().fold(n, |acc, (p, _)| acc / p * (p - 1)))
}

/// The smallest prime strictly greater than `n`.
pub fn next_prime(n: u64) -> Option<u64> {
    let mut k = n.checked_add(1)?;
    while !is_prime(k) {
        k = k.checked_add(1)?;
    }
    Some(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primes_and_factors() {
        assert!(is_prime(97) && !is_prime(1) && !is_prime(91) && is_prime(2_147_483_647));
        assert!(is_prime(18_446_744_073_709_551_557));
        assert_eq!(factor(360), Some(vec![(2, 3), (3, 2), (5, 1)]));
        assert_eq!(factor(1_000_000_007 * 998_244_353), Some(vec![(998_244_353, 1), (1_000_000_007, 1)]));
        assert_eq!(totient(36), Some(12));
        assert_eq!(next_prime(97), Some(101));
    }
}
