//! Pure vector math helpers. No allocations on the hot path.

/// Dot product of two equal-length slices.
/// Callers are responsible for length validation; this panics on mismatch.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len(), "dot: length mismatch");
    let mut sum = 0.0f32;
    for i in 0..a.len() {
        sum += a[i] * b[i];
    }
    sum
}

/// L2 norm (Euclidean length).
pub fn norm(v: &[f32]) -> f32 {
    dot(v, v).sqrt()
}

/// Normalize in place to unit length.
/// Returns the original norm. Returns 0.0 if the vector was zero-length
/// (caller should treat this as an error; we leave the vector untouched).
pub fn normalize(v: &mut [f32]) -> f32 {
    let n = norm(v);
    if n > 0.0 && n.is_finite() {
        let inv = 1.0 / n;
        for x in v.iter_mut() {
            *x *= inv;
        }
    }
    n
}

/// Cosine similarity. Returns 0.0 if either vector is zero-length.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let na = norm(a);
    let nb = norm(b);
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot(a, b) / (na * nb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_basic() {
        assert_eq!(dot(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]), 32.0);
    }

    #[test]
    fn norm_basic() {
        assert!((norm(&[3.0, 4.0]) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_unit_length() {
        let mut v = [3.0f32, 4.0];
        let n = normalize(&mut v);
        assert!((n - 5.0).abs() < 1e-6);
        assert!((norm(&v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_zero_is_noop() {
        let mut v = [0.0f32, 0.0, 0.0];
        let n = normalize(&mut v);
        assert_eq!(n, 0.0);
        assert_eq!(v, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn cosine_identical_is_one() {
        let a = [1.0f32, 2.0, 3.0];
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = [1.0f32, 0.0];
        let b = [0.0f32, 1.0];
        assert!(cosine(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn cosine_opposite_is_minus_one() {
        let a = [1.0f32, 0.0];
        let b = [-1.0f32, 0.0];
        assert!((cosine(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_zero_vector_is_zero() {
        let a = [0.0f32, 0.0];
        let b = [1.0f32, 1.0];
        assert_eq!(cosine(&a, &b), 0.0);
    }
}
