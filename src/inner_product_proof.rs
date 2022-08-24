// Copyright (c) 2022, MaidSafe.
// All rights reserved.
//
// This SAFE Network Software is licensed under the MIT license.
// Please see the LICENSE file for more details.

#![allow(non_snake_case)]
#![cfg_attr(feature = "docs", doc(include = "../docs/inner-product-protocol.md"))]

extern crate alloc;

use alloc::borrow::Borrow;
use alloc::vec::Vec;

use blstrs::{G1Projective, Scalar};
use core::iter;
use group::ff::Field;
use merlin::Transcript;

use crate::errors::ProofError;
use crate::transcript::TranscriptProtocol;

#[derive(Eq, PartialEq, Clone, Debug)]
pub struct InnerProductProof {
    pub(crate) L_vec: Vec<G1Projective>,
    pub(crate) R_vec: Vec<G1Projective>,
    pub(crate) a: Scalar,
    pub(crate) b: Scalar,
}

impl InnerProductProof {
    /// Create an inner-product proof.
    ///
    /// The proof is created with respect to the bases \\(G\\), \\(H'\\),
    /// where \\(H'\_i = H\_i \cdot \texttt{Hprime\\_factors}\_i\\).
    ///
    /// The `verifier` is passed in as a parameter so that the
    /// challenges depend on the *entire* transcript (including parent
    /// protocols).
    ///
    /// The lengths of the vectors must all be the same, and must all be
    /// either 0 or a power of 2.
    pub fn create(
        transcript: &mut Transcript,
        Q: &G1Projective,
        G_factors: &[Scalar],
        H_factors: &[Scalar],
        mut G_vec: Vec<G1Projective>,
        mut H_vec: Vec<G1Projective>,
        mut a_vec: Vec<Scalar>,
        mut b_vec: Vec<Scalar>,
    ) -> Result<InnerProductProof, ProofError> {
        // Create slices G, H, a, b backed by their respective
        // vectors.  This lets us reslice as we compress the lengths
        // of the vectors in the main loop below.
        let mut G = &mut G_vec[..];
        let mut H = &mut H_vec[..];
        let mut a = &mut a_vec[..];
        let mut b = &mut b_vec[..];

        let mut n = G.len();

        // All of the input vectors must have the same length.
        assert_eq!(G.len(), n);
        assert_eq!(H.len(), n);
        assert_eq!(a.len(), n);
        assert_eq!(b.len(), n);
        assert_eq!(G_factors.len(), n);
        assert_eq!(H_factors.len(), n);

        // All of the input vectors must have a length that is a power of two.
        assert!(n.is_power_of_two());

        transcript.innerproduct_domain_sep(n as u64);

        let lg_n = n.next_power_of_two().trailing_zeros() as usize;
        let mut L_vec = Vec::with_capacity(lg_n);
        let mut R_vec = Vec::with_capacity(lg_n);

        // If it's the first iteration, unroll the Hprime = H*y_inv scalar mults
        // into multiscalar muls, for performance.
        if n != 1 {
            n = n / 2;
            let (a_L, a_R) = a.split_at_mut(n);
            let (b_L, b_R) = b.split_at_mut(n);
            let (G_L, G_R) = G.split_at_mut(n);
            let (H_L, H_R) = H.split_at_mut(n);

            let c_L = inner_product(&a_L, &b_R);
            let c_R = inner_product(&a_R, &b_L);

            let L = a_L
                .iter()
                .zip(G_factors[n..2 * n].into_iter())
                .map(|(a_L_i, g)| a_L_i * g)
                .chain(
                    b_R.iter()
                        .zip(H_factors[0..n].into_iter())
                        .map(|(b_R_i, h)| b_R_i * h),
                )
                .chain(iter::once(c_L))
                .zip(G_R.iter().chain(H_L.iter()).chain(iter::once(Q)))
                .map(|(s, P)| P * s)
                .sum();

            let R = a_R
                .iter()
                .zip(G_factors[0..n].into_iter())
                .map(|(a_R_i, g)| a_R_i * g)
                .chain(
                    b_L.iter()
                        .zip(H_factors[n..2 * n].into_iter())
                        .map(|(b_L_i, h)| b_L_i * h),
                )
                .chain(iter::once(c_R))
                .zip(G_L.iter().chain(H_R.iter()).chain(iter::once(Q)))
                .map(|(s, P)| P * s)
                .sum();

            L_vec.push(L);
            R_vec.push(R);

            transcript.append_point(b"L", &L);
            transcript.append_point(b"R", &R);

            let u = transcript.challenge_scalar(b"u");
            let u_inv: Scalar = Option::from(u.invert()).ok_or(ProofError::FormatError)?;

            for i in 0..n {
                a_L[i] = a_L[i] * u + a_R[i] * u_inv;
                b_L[i] = b_L[i] * u_inv + b_R[i] * u;
                G_L[i] = G_L[i] * (u_inv * G_factors[i]) + G_R[i] * (u * G_factors[n + i]);
                H_L[i] = H_L[i] * (u * H_factors[i]) + H_R[i] * (u_inv * H_factors[n + i]);
            }

            a = a_L;
            b = b_L;
            G = G_L;
            H = H_L;
        }

        while n != 1 {
            n = n / 2;
            let (a_L, a_R) = a.split_at_mut(n);
            let (b_L, b_R) = b.split_at_mut(n);
            let (G_L, G_R) = G.split_at_mut(n);
            let (H_L, H_R) = H.split_at_mut(n);

            let c_L = inner_product(&a_L, &b_R);
            let c_R = inner_product(&a_R, &b_L);

            let L = a_L
                .iter()
                .chain(b_R.iter())
                .chain(iter::once(&c_L))
                .zip(G_R.iter().chain(H_L.iter()).chain(iter::once(Q)))
                .map(|(s, P)| P * s)
                .sum();

            let R = a_R
                .iter()
                .chain(b_L.iter())
                .chain(iter::once(&c_R))
                .zip(G_L.iter().chain(H_R.iter()).chain(iter::once(Q)))
                .map(|(s, P)| P * s)
                .sum();

            L_vec.push(L);
            R_vec.push(R);

            transcript.append_point(b"L", &L);
            transcript.append_point(b"R", &R);

            let u = transcript.challenge_scalar(b"u");
            let u_inv: Scalar = Option::from(u.invert()).ok_or(ProofError::FormatError)?;

            for i in 0..n {
                a_L[i] = a_L[i] * u + a_R[i] * u_inv;
                b_L[i] = b_L[i] * u_inv + b_R[i] * u;
                G_L[i] = G_L[i] * u_inv + G_R[i] * u;
                H_L[i] = H_L[i] * u + H_R[i] * u_inv;
            }

            a = a_L;
            b = b_L;
            G = G_L;
            H = H_L;
        }

        Ok(InnerProductProof {
            L_vec: L_vec,
            R_vec: R_vec,
            a: a[0],
            b: b[0],
        })
    }

    /// Computes three vectors of verification scalars \\([u\_{i}^{2}]\\), \\([u\_{i}^{-2}]\\) and \\([s\_{i}]\\) for combined multiscalar multiplication
    /// in a parent protocol. See [inner product protocol notes](index.html#verification-equation) for details.
    /// The verifier must provide the input length \\(n\\) explicitly to avoid unbounded allocation within the inner product proof.
    pub(crate) fn verification_scalars(
        &self,
        n: usize,
        transcript: &mut Transcript,
    ) -> Result<(Vec<Scalar>, Vec<Scalar>, Vec<Scalar>), ProofError> {
        let lg_n = self.L_vec.len();
        if lg_n >= 32 {
            // 4 billion multiplications should be enough for anyone
            // and this check prevents overflow in 1<<lg_n below.
            return Err(ProofError::VerificationError);
        }
        if n != (1 << lg_n) {
            return Err(ProofError::VerificationError);
        }

        transcript.innerproduct_domain_sep(n as u64);

        // 1. Recompute x_k,...,x_1 based on the proof transcript

        let mut challenges = Vec::with_capacity(lg_n);
        for (L, R) in self.L_vec.iter().zip(self.R_vec.iter()) {
            transcript.validate_and_append_point(b"L", L)?;
            transcript.validate_and_append_point(b"R", R)?;
            challenges.push(transcript.challenge_scalar(b"u"));
        }

        // 2. Compute 1/(u_k...u_1) and 1/u_k, ..., 1/u_1

        // TODO: very non-optimal code, check if blst has the equivalent Scalar::batch_invert function
        // https://docs.rs/curve25519-dalek-ng/4.1.1/curve25519_dalek_ng/scalar/struct.Scalar.html#method.batch_invert
        let mut challenges_inv = challenges
            .clone()
            .into_iter()
            .map(|u| Option::from(u.invert()).ok_or(ProofError::FormatError))
            .collect::<Result<Vec<_>, _>>()?;
        // todo: replace fold() with product() when supported in blstrs
        let allinv = challenges_inv
            .iter()
            .fold(Scalar::one(), |product, x| product * x);

        // 3. Compute u_i^2 and (1/u_i)^2

        for i in 0..lg_n {
            // XXX missing square fn upstream
            challenges[i] = challenges[i] * challenges[i];
            challenges_inv[i] = challenges_inv[i] * challenges_inv[i];
        }
        let challenges_sq = challenges;
        let challenges_inv_sq = challenges_inv;

        // 4. Compute s values inductively.

        let mut s = Vec::with_capacity(n);
        s.push(allinv);
        for i in 1..n {
            let lg_i = (32 - 1 - (i as u32).leading_zeros()) as usize;
            let k = 1 << lg_i;
            // The challenges are stored in "creation order" as [u_k,...,u_1],
            // so u_{lg(i)+1} = is indexed by (lg_n-1) - lg_i
            let u_lg_i_sq = challenges_sq[(lg_n - 1) - lg_i];
            s.push(s[i - k] * u_lg_i_sq);
        }

        Ok((challenges_sq, challenges_inv_sq, s))
    }

    /// This method is for testing that proof generation work,
    /// but for efficiency the actual protocols would use `verification_scalars`
    /// method to combine inner product verification with other checks
    /// in a single multiscalar multiplication.
    #[allow(dead_code)]
    pub fn verify<IG, IH>(
        &self,
        n: usize,
        transcript: &mut Transcript,
        G_factors: IG,
        H_factors: IH,
        P: &G1Projective,
        Q: &G1Projective,
        G: &[G1Projective],
        H: &[G1Projective],
    ) -> Result<(), ProofError>
    where
        IG: IntoIterator,
        IG::Item: Borrow<Scalar>,
        IH: IntoIterator,
        IH::Item: Borrow<Scalar>,
    {
        let (u_sq, u_inv_sq, s) = self.verification_scalars(n, transcript)?;

        let g_times_a_times_s = G_factors
            .into_iter()
            .zip(s.iter())
            .map(|(g_i, s_i)| (self.a * s_i) * g_i.borrow())
            .take(G.len());

        // 1/s[i] is s[!i], and !i runs from n-1 to 0 as i runs from 0 to n-1
        let inv_s = s.iter().rev();

        let h_times_b_div_s = H_factors
            .into_iter()
            .zip(inv_s)
            .map(|(h_i, s_i_inv)| (self.b * s_i_inv) * h_i.borrow());

        let neg_u_sq = u_sq.iter().map(|ui| -ui);
        let neg_u_inv_sq = u_inv_sq.iter().map(|ui| -ui);

        let scalars = iter::once(self.a * self.b)
            .chain(g_times_a_times_s)
            .chain(h_times_b_div_s)
            .chain(neg_u_sq)
            .chain(neg_u_inv_sq);
        let points = iter::once(Q)
            .chain(G.iter())
            .chain(H.iter())
            .chain(self.L_vec.iter())
            .chain(self.R_vec.iter());
        let expect_P: G1Projective = scalars.zip(points).map(|(s, P)| P * s).sum();

        if expect_P == *P {
            Ok(())
        } else {
            Err(ProofError::VerificationError)
        }
    }

    /// Returns the size in bytes required to serialize the inner
    /// product proof.
    ///
    /// For vectors of length `n` the proof size is
    /// \\(32 \cdot (2\lg n+2)\\) bytes.
    pub fn serialized_size(&self) -> usize {
        (self.L_vec.len() * 2) * 48 + 2 * 32
    }

    /// Serializes the proof into a byte array of \\(2n+2\\) 32-byte elements.
    /// The layout of the inner product proof is:
    /// * \\(n\\) pairs of compressed Ristretto points \\(L_0, R_0 \dots, L_{n-1}, R_{n-1}\\),
    /// * two scalars \\(a, b\\).
    #[allow(dead_code)]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.serialized_size());
        for (l, r) in self.L_vec.iter().zip(self.R_vec.iter()) {
            buf.extend_from_slice(&l.to_compressed());
            buf.extend_from_slice(&r.to_compressed());
        }
        buf.extend_from_slice(&self.a.to_bytes_le());
        buf.extend_from_slice(&self.b.to_bytes_le());
        buf
    }

    /// Converts the proof into a byte iterator over serialized view of the proof.
    /// The layout of the inner product proof is:
    /// * \\(n\\) pairs of compressed Ristretto points \\(L_0, R_0 \dots, L_{n-1}, R_{n-1}\\),
    /// * two scalars \\(a, b\\).
    #[inline]
    pub(crate) fn to_bytes_iter(&self) -> impl Iterator<Item = u8> + '_ {
        self.L_vec
            .iter()
            .zip(self.R_vec.iter())
            .flat_map(|(l, r)| {
                l.to_compressed()
                    .iter()
                    .copied()
                    .chain(r.to_compressed())
                    .collect::<Vec<_>>()
            })
            .chain(self.a.to_bytes_le())
            .chain(self.b.to_bytes_le())
    }

    /// Deserializes the proof from a byte slice.
    /// Returns an error in the following cases:
    /// * the slice does not have \\(2n\\) 48-byte elements + 2 32-byte elements,
    /// * \\(n\\) is larger or equal to 32 (proof is too big),
    /// * any of \\(2n\\) points are not valid compressed bls12-381 G1 points,
    /// * any of 2 scalars are not canonical scalars modulo bls12-381 G1 group order.
    pub fn from_bytes(slice: &[u8]) -> Result<InnerProductProof, ProofError> {
        let b = slice.len();
        if b < 2 * 32 {
            return Err(ProofError::FormatError);
        }
        if (b - 32 * 2) % 48 != 0 {
            // last two elements are scalars,
            return Err(ProofError::FormatError);
        }
        let num_points = (b - 32 * 2) / 48;
        if num_points % 2 != 0 {
            return Err(ProofError::FormatError);
        }

        let lg_n = num_points / 2;
        if lg_n >= 32 {
            return Err(ProofError::FormatError);
        }

        use crate::util::{read32, read48};

        let mut L_vec: Vec<G1Projective> = Vec::with_capacity(lg_n);
        let mut R_vec: Vec<G1Projective> = Vec::with_capacity(lg_n);
        for i in 0..lg_n {
            let pos = 2 * i * 48;
            L_vec.push(
                Option::from(G1Projective::from_compressed(&read48(&slice[pos..])))
                    .ok_or(ProofError::FormatError)?,
            );
            R_vec.push(
                Option::from(G1Projective::from_compressed(&read48(&slice[pos + 48..])))
                    .ok_or(ProofError::FormatError)?,
            );
        }

        let pos = 2 * lg_n * 48;
        let a = Option::from(Scalar::from_bytes_le(&read32(&slice[pos..])))
            .ok_or(ProofError::FormatError)?;
        let b = Option::from(Scalar::from_bytes_le(&read32(&slice[pos + 32..])))
            .ok_or(ProofError::FormatError)?;

        Ok(InnerProductProof { L_vec, R_vec, a, b })
    }
}

/// Computes an inner product of two vectors
/// \\[
///    {\langle {\mathbf{a}}, {\mathbf{b}} \rangle} = \sum\_{i=0}^{n-1} a\_i \cdot b\_i.
/// \\]
/// Panics if the lengths of \\(\mathbf{a}\\) and \\(\mathbf{b}\\) are not equal.
pub fn inner_product(a: &[Scalar], b: &[Scalar]) -> Scalar {
    let mut out = Scalar::zero();
    if a.len() != b.len() {
        panic!("inner_product(a,b): lengths of vectors do not match");
    }
    for i in 0..a.len() {
        out += a[i] * b[i];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::util;

    fn test_helper_create(n: usize) {
        let mut rng = rand::thread_rng();

        use crate::generators::BulletproofGens;
        let bp_gens = BulletproofGens::new(n, 1);
        let G: Vec<G1Projective> = bp_gens.share(0).G(n).cloned().collect();
        let H: Vec<G1Projective> = bp_gens.share(0).H(n).cloned().collect();

        // Q would be determined upstream in the protocol, so we pick a random one.
        let Q = G1Projective::hash_to_curve(b"test point", b"tests", &[]);

        // a and b are the vectors for which we want to prove c = <a,b>
        let a: Vec<_> = (0..n).map(|_| Scalar::random(&mut rng)).collect();
        let b: Vec<_> = (0..n).map(|_| Scalar::random(&mut rng)).collect();
        let c = inner_product(&a, &b);

        let G_factors: Vec<Scalar> = iter::repeat(Scalar::one()).take(n).collect();

        // y_inv is (the inverse of) a random challenge
        let y_inv = Scalar::random(&mut rng);
        let H_factors: Vec<Scalar> = util::exp_iter(y_inv).take(n).collect();

        // P would be determined upstream, but we need a correct P to check the proof.
        //
        // To generate P = <a,G> + <b,H'> + <a,b> Q, compute
        //             P = <a,G> + <b',H> + <a,b> Q,
        // where b' = b \circ y^(-n)
        let b_prime = b.iter().zip(util::exp_iter(y_inv)).map(|(bi, yi)| bi * yi);
        // a.iter() has Item=&Scalar, need Item=Scalar to chain with b_prime
        let a_prime = a.iter().cloned();

        let P: G1Projective = a_prime
            .chain(b_prime)
            .chain(iter::once(c))
            .zip(G.iter().chain(H.iter()).chain(iter::once(&Q)))
            .map(|(a, P)| P * a)
            .sum();

        let mut verifier = Transcript::new(b"innerproducttest");
        let proof = InnerProductProof::create(
            &mut verifier,
            &Q,
            &G_factors,
            &H_factors,
            G.clone(),
            H.clone(),
            a.clone(),
            b.clone(),
        )
        .unwrap();

        let mut verifier = Transcript::new(b"innerproducttest");
        assert!(proof
            .verify(
                n,
                &mut verifier,
                iter::repeat(Scalar::one()).take(n),
                util::exp_iter(y_inv).take(n),
                &P,
                &Q,
                &G,
                &H
            )
            .is_ok());

        let proof = InnerProductProof::from_bytes(proof.to_bytes().as_slice()).unwrap();
        let mut verifier = Transcript::new(b"innerproducttest");
        assert!(proof
            .verify(
                n,
                &mut verifier,
                iter::repeat(Scalar::one()).take(n),
                util::exp_iter(y_inv).take(n),
                &P,
                &Q,
                &G,
                &H
            )
            .is_ok());
    }

    #[test]
    fn make_ipp_1() {
        test_helper_create(1);
    }

    #[test]
    fn make_ipp_2() {
        test_helper_create(2);
    }

    #[test]
    fn make_ipp_4() {
        test_helper_create(4);
    }

    #[test]
    fn make_ipp_32() {
        test_helper_create(32);
    }

    #[test]
    fn make_ipp_64() {
        test_helper_create(64);
    }

    #[test]
    fn test_inner_product() {
        let a = vec![
            Scalar::from(1u64),
            Scalar::from(2u64),
            Scalar::from(3u64),
            Scalar::from(4u64),
        ];
        let b = vec![
            Scalar::from(2u64),
            Scalar::from(3u64),
            Scalar::from(4u64),
            Scalar::from(5u64),
        ];
        assert_eq!(Scalar::from(40u64), inner_product(&a, &b));
    }
}
