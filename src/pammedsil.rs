use crate::arrayadapter::ArrayAdapter;
use crate::fastermsc::{do_swap, initial_assignment};
use crate::util::*;
use core::ops::AddAssign;
use num_traits::{Signed, Zero, Float, FromPrimitive};
use std::convert::From;

#[inline]
fn _loss<N, L>(a: N, b: N) -> L
	where
		N: Zero,
		L: Float + From<N> + FromPrimitive,
{
	if N::is_zero(&a) || N::is_zero(&b) { L::zero() } else { <L as From<N>>::from(a) / <L as From<N>>::from(b) } 
}

/// Run the original PAMMEDSIL SWAP algorithm (no initialization, but given initial medoids).
///
/// This is provided for academic reasons to see the performance difference.
/// Quality-wise, FasterMSC is not worse on average, but much faster.
/// FastMSC is supposed to do the same swaps, and find the same result, but faster.
///
/// * type `M` - matrix data type such as `ndarray::Array2` or `kmedoids::arrayadapter::LowerTriangle`
/// * type `N` - number data type such as `u32` or `f64`
/// * type `L` - number data type such as `i64` or `f64` for the loss (must be signed)
/// * `mat` - a pairwise distance matrix
/// * `med` - the list of medoids
/// * `maxiter` - the maximum number of iterations allowed
///
/// returns a tuple containing:
/// * the final loss
/// * the final cluster assignment
/// * the number of iterations needed
/// * the number of swaps performed
///
/// ## Panics
///
/// * panics when the dissimilarity matrix is not square
/// * panics when k is 0 or larger than N
///
/// ## Example
/// Given a dissimilarity matrix of size 4 x 4, use:
/// ```
/// let data = ndarray::arr2(&[[0,1,2,3],[1,0,4,5],[2,4,0,6],[3,5,6,0]]);
/// let mut meds = kmedoids::random_initialization(4, 2, &mut rand::thread_rng());
/// let (loss, assi, n_iter, n_swap): (f64, _, _, _) = kmedoids::pamsil_swap(&data, &mut meds, 100);
/// println!("Loss is: {}", loss);
/// ```
pub fn pammedsil_swap<M, N, L>(
	mat: &M,
	med: &mut Vec<usize>,
	maxiter: usize,
) -> (L, Vec<usize>, usize, usize)
	where
		N: Zero + PartialOrd + Copy,
		L: Float + Signed + AddAssign + From<N> + std::convert::From<u32> + FromPrimitive + std::fmt::Display,
		M: ArrayAdapter<N>,
{
	let (loss, mut data) = initial_assignment(mat, med);
	pammedsil_optimize(mat, med, &mut data, maxiter, loss)
}

/// Run the original PAM BUILD algorithm combined with the PAMMEDSIL SWAP.
///
/// * type `M` - matrix data type such as `ndarray::Array2` or `kmedoids::arrayadapter::LowerTriangle`
/// * type `N` - number data type such as `u32` or `f64`
/// * type `L` - number data type such as `i64` or `f64` for the loss (must be signed)
/// * `mat` - a pairwise distance matrix
/// * `k` - the number of medoids to pick
/// * `maxiter` - the maximum number of iterations allowed
///
/// returns a tuple containing:
/// * the final loss
/// * the final cluster assignment
/// * the final medoids
/// * the number of iterations needed
/// * the number of swaps performed
///
/// ## Panics
///
/// * panics when the dissimilarity matrix is not square
/// * panics when k is 0 or larger than N
///
/// ## Example
/// Given a dissimilarity matrix of size 4 x 4, use:
/// ```
/// let data = ndarray::arr2(&[[0,1,2,3],[1,0,4,5],[2,4,0,6],[3,5,6,0]]);
/// let (loss, assi, meds, n_iter, n_swap): (f64, _, _, _, _) = kmedoids::pamsil(&data, 2, 100);
/// println!("Loss is: {}", loss);
/// ```
pub fn pammedsil<M, N, L>(mat: &M, k: usize, maxiter: usize) -> (L, Vec<usize>, Vec<usize>, usize, usize)
	where
		N: Zero + PartialOrd + Copy,
		L: Float + Signed + AddAssign + From<N> + std::convert::From<u32> + FromPrimitive + std::fmt::Display,
		M: ArrayAdapter<N>,
{
	let n = mat.len();
	assert!(mat.is_square(), "Dissimilarity matrix is not square");
	assert!(n <= u32::MAX as usize, "N is too large");
	assert!(k > 0 && k < u32::MAX as usize, "invalid N");
	assert!(k <= n, "k must be at most N");
	let mut meds = Vec::<usize>::with_capacity(k);
	let mut data = Vec::<Reco<N>>::with_capacity(n);
	let loss = pammedsil_build_initialize(mat, &mut meds, &mut data, k);
	let (nloss, assi, n_iter, n_swap) = pammedsil_optimize(mat, &mut meds, &mut data, maxiter, loss);
	(nloss, assi, meds, n_iter, n_swap) // also return medoids
}

/// Main optimization function of PAMMEDSIL, not exposed (use pammedsil_swap or pammedsil)
fn pammedsil_optimize<M, N, L>(
	mat: &M,
	med: &mut Vec<usize>,
	data: &mut Vec<Reco<N>>,
	maxiter: usize,
	mut loss: L,
) -> (L, Vec<usize>, usize, usize)
	where
		N: Zero + PartialOrd + Copy,
		L: Float + Signed + AddAssign + From<N> + std::convert::From<u32> + FromPrimitive + std::fmt::Display,
		M: ArrayAdapter<N>,
{
	let (n, k) = (mat.len(), med.len());
	if k == 1 {
		let assi = vec![0; n];
		let (swapped, loss) = choose_medoid_within_partition::<M, N, L>(mat, &assi, med, 0);
		return (loss, assi, 1, if swapped { 1 } else { 0 });
	}
	debug_assert_assignment_th(mat, med, data);
	let (mut n_swaps, mut iter) = (0, 0);
	while iter < maxiter {
		iter += 1;
		let mut best = (L::zero(), k, usize::MAX);
		for j in 0..n {
			if j == med[data[j].near.i as usize] {
				continue; // This already is a medoid
			}
			let (change, b): (L, usize) = if k == 2 {
				find_best_swap_pammedsil_k2(mat, med, data, j)
			} else {
				find_best_swap_pammedsil(mat, med, data, j)
			};
			if change <= best.0 {
				continue; // No improvement
			}
			best = (change, b, j);
		}
		if best.0 > L::zero() {
			n_swaps += 1;
			// perform the swap
			let newloss : L = do_swap(mat, med, data, best.1, best.2);
			if newloss >= loss {
				break; // Probably numerically unstable now.
			}
			loss = newloss;
		} else {
			break; // No improvement, or NaN.
		}
	}
	let assi = data.iter().map(|x| x.near.i as usize).collect();
	loss = L::one() - loss / <L as From<u32>>::from(n as u32);
	(loss, assi, iter, n_swaps)
}

/// Find the best swap for object j
#[inline]
fn find_best_swap_pammedsil<M, N, L>(mat: &M, med: &[usize], data: &[Reco<N>], j: usize) -> (L, usize)
	where
		N: Zero + PartialOrd + Copy,
		L: Float + AddAssign + From<N> + FromPrimitive + std::fmt::Display,
		M: ArrayAdapter<N>,
{
	let recj = &data[j];
	let mut best = (L::zero(), usize::MAX);
	for (m, _) in med.iter().enumerate() {
		let mut acc: L = _loss::<N, L>(recj.near.d, recj.seco.d); // j becomes medoid
		for (o, reco) in data.iter().enumerate() {
			if o == j {
				continue;
			}
			let doj = mat.get(o, j);
			// Current medoid is being replaced:
			if reco.near.i as usize == m {
				if doj < reco.seco.d {
					// Assign to new medoid:
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(doj, reco.seco.d);
				} else if doj < reco.third.d {
					// Assign to second nearest instead:
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(reco.seco.d, doj);
				} else {
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(reco.seco.d, reco.third.d);
				}
			} else if reco.seco.i as usize == m  {
				if doj < reco.near.d {
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(doj, reco.near.d);
				} else if doj < reco.third.d {
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(reco.near.d, doj);
				} else {
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(reco.near.d, reco.third.d);
				}
			} else {
				if doj < reco.near.d {
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(doj, reco.near.d);
				} else if doj < reco.seco.d {
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(reco.near.d, doj);
				}
			}
		}
		if acc > best.0 {
			best = (acc, m);
		}
	}
	best
}

/// Find the best swap for object j
#[inline]
fn find_best_swap_pammedsil_k2<M, N, L>(mat: &M, med: &[usize], data: &[Reco<N>], j: usize) -> (L, usize)
	where
		N: Zero + PartialOrd + Copy,
		L: Float + AddAssign + From<N> + FromPrimitive + std::fmt::Display,
		M: ArrayAdapter<N>,
{
	let recj = &data[j];
	let mut best = (L::zero(), usize::MAX);
	for (m, _) in med.iter().enumerate() {
		let mut acc: L = _loss::<N, L>(recj.near.d, recj.seco.d); // j becomes medoid
		for (o, reco) in data.iter().enumerate() {
			if o == j {
				continue;
			}
			let doj = mat.get(o, j);
			// Current medoid is being replaced:
			if reco.near.i as usize == m {
				if doj < reco.seco.d {
					// Assign to new medoid:
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(doj, reco.seco.d);
				} else {
					// Assign to second nearest instead:
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(reco.seco.d, doj);
				}
			} else if reco.seco.i as usize == m  {
				if doj < reco.near.d {
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(doj, reco.near.d);
				} else {
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(reco.near.d, doj);
				}
			} else {
				if doj < reco.near.d {
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(doj, reco.near.d);
				} else if doj < reco.seco.d {
					acc += _loss::<N, L>(reco.near.d, reco.seco.d) - _loss::<N, L>(reco.near.d, doj);
				}
			}
		}
		if acc > best.0 {
			best = (acc, m);
		}
	}
	best
}

/// Not exposed. Use pammedsil_build or pammedsil.
fn pammedsil_build_initialize<M, N, L>(
	mat: &M,
	meds: &mut Vec<usize>,
	data: &mut Vec<Reco<N>>,
	k: usize,
) -> L
	where
		N: Zero + PartialOrd + Copy,
		L: Float + Signed + AddAssign + From<N> + FromPrimitive + std::fmt::Display,
		M: ArrayAdapter<N>,
{
	let n = mat.len();
	// choose first medoid
	let mut best = (L::zero(), k);
	for i in 0..n {
		let mut sum = L::zero();
		for j in 0..n {
			if j != i {
				sum += <L as From<N>>::from(mat.get(j, i));
			}
		}
		if i == 0 || sum < best.0 {
			best = (sum, i);
		}
	}
	let mut loss = best.0;
	meds.push(best.1);
	for j in 0..n {
		data.push(Reco::new(0, mat.get(j, best.1), u32::MAX, N::zero(), u32::MAX, N::zero()));
	}
	// choose remaining medoids
	for l in 1..k {
		best = (L::zero(), k);
		for (i, _) in data.iter().enumerate().skip(1) {
			let mut sum = -<L as From<N>>::from(data[i].near.d);
			for (j, dj) in data.iter().enumerate() {
				if j != i {
					let d = mat.get(j, i);
					if d < dj.near.d {
						sum += <L as From<N>>::from(d) - <L as From<N>>::from(dj.near.d)
					}
				}
			}
			if i == 0 || sum < best.0 {
				best = (sum, i);
			}
		}
		if best.0 >= L::zero() { break; } // No more improvement, duplicates
		// Update assignments:
		loss = L::zero();
		for (j, recj) in data.iter_mut().enumerate() {
			if j == best.1 {
				recj.third = recj.seco;
				recj.seco = recj.near;
				recj.near = DistancePair::new(l as u32, N::zero());
			} else {
				let dj = mat.get(j, best.1);
				if dj < recj.near.d {
					recj.third = recj.seco;
					recj.seco = recj.near;
					recj.near = DistancePair::new(l as u32, dj);
				} else if recj.seco.i == u32::MAX || dj < recj.seco.d {
					recj.third = recj.seco;
					recj.seco = DistancePair::new(l as u32, dj);
				} else if recj.third.i == u32::MAX || dj < recj.third.d {
					recj.third = DistancePair::new(l as u32, dj);
				}
			}
			loss += _loss::<N, L>(recj.near.d, recj.seco.d);
		}
		meds.push(best.1);
	}
	loss
}

#[cfg(test)]
mod tests {
	// TODO: use a larger, much more interesting example.
	use crate::{
		arrayadapter::LowerTriangle, pammedsil, pammedsil_swap, silhouette, medoid_silhouette, util::assert_array,
	};

	#[test]
	fn test_pammedsil() {
		let data = LowerTriangle {
			n: 5,
			data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 1],
		};
		let (loss, assi, meds, n_iter, n_swap): (f64, _, _, _, _) = pammedsil(&data, 3, 10);
		let (sil, _): (f64, _) = silhouette(&data, &assi, false);
		let (msil, _): (f64, _) = medoid_silhouette(&data, &meds, false);
		println!("PAMMedSil: {:?} {:?} {:?} {:?} {:?} {:?}", loss, n_iter, n_swap, sil, assi, meds);
		assert_eq!(n_swap, 0, "swaps not as expected");
		assert_eq!(n_iter, 1, "iterations not as expected");
		assert_eq!(loss, 0.9047619047619048, "loss not as expected");
		assert_eq!(msil, 0.9047619047619048, "Medoid Silhouettte not as expected");
		assert_array(assi, vec![0, 0, 2, 1, 1], "assignment not as expected");
		assert_array(meds, vec![0, 3, 2], "medoids not as expected");
		assert_eq!(sil, 0.5622222222222222, "Silhouette not as expected");
	}

	#[test]
	fn testpammedsil_simple() {
		let data = LowerTriangle {
			n: 5,
			data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 1],
		};
		let mut meds = vec![0, 1, 2];
		let (loss, assi, n_iter, n_swap): (f64, _, _, _) = pammedsil_swap(&data, &mut meds, 10);
		let (sil, _): (f64, _) = silhouette(&data, &assi, false);
		let (msil, _): (f64, _) = medoid_silhouette(&data, &meds, false);
		println!("PAMMedSil: {:?} {:?} {:?} {:?} {:?} {:?}", loss, n_iter, n_swap, sil, assi, meds);
		assert_eq!(loss, 0.9047619047619048, "loss not as expected");
		assert_eq!(msil, 0.9047619047619048, "Medoid Silhouette not as expected");
		assert_eq!(n_swap, 1, "swaps not as expected");
		assert_eq!(n_iter, 2, "iterations not as expected");
		assert_array(assi, vec![0, 0, 2, 1, 1], "assignment not as expected");
		assert_array(meds, vec![0, 3, 2], "medoids not as expected");
		assert_eq!(sil, 0.5622222222222222, "Silhouette not as expected");
	}

	#[test]
	fn testpammedsil_simple2() {
		let data = LowerTriangle {
			n: 5,
			data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 1],
		};
		let mut meds = vec![0, 1];
		let (loss, assi, n_iter, n_swap): (f64, _, _, _) = pammedsil_swap(&data, &mut meds, 10);
		let (sil, _): (f64, _) = silhouette(&data, &assi, false);
		let (msil, _): (f64, _) = medoid_silhouette(&data, &meds, false);
		println!("PAMMedSil: {:?} {:?} {:?} {:?} {:?} {:?}", loss, n_iter, n_swap, sil, assi, meds);
		assert_eq!(loss, 0.8805555555555555, "loss not as expected");
		assert_eq!(msil, 0.8805555555555555, "Medoid Silhouette not as expected");
		assert_eq!(n_swap, 1, "swaps not as expected");
		assert_eq!(n_iter, 2, "iterations not as expected");
		assert_array(assi, vec![0, 0, 0, 1, 1], "assignment not as expected");
		assert_array(meds, vec![0, 4], "medoids not as expected");
		assert_eq!(sil, 0.7522494172494172, "Silhouette not as expected");
	}
}
