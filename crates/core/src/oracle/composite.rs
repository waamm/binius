// Copyright 2024-2025 Irreducible Inc.

use std::sync::Arc;

use binius_field::TowerField;
use binius_math::CompositionPoly;
use binius_utils::bail;

use crate::oracle::{Error, MultilinearPolyOracle, OracleId};

#[derive(Debug, Clone)]
pub struct CompositePolyOracle<F: TowerField> {
	n_vars: usize,
	inner: Vec<MultilinearPolyOracle<F>>,
	composition: Arc<dyn CompositionPoly<F>>,
}

impl<F: TowerField> CompositePolyOracle<F> {
	pub fn new<C: CompositionPoly<F> + 'static>(
		n_vars: usize,
		inner: Vec<MultilinearPolyOracle<F>>,
		composition: C,
	) -> Result<Self, Error> {
		if inner.len() != composition.n_vars() {
			bail!(Error::CompositionMismatch);
		}
		for poly in &inner {
			if poly.n_vars() != n_vars {
				bail!(Error::IncorrectNumberOfVariables { expected: n_vars });
			}
		}
		Ok(Self {
			n_vars,
			inner,
			composition: Arc::new(composition),
		})
	}

	pub fn max_individual_degree(&self) -> usize {
		// Maximum individual degree of the multilinear composite equals composition degree
		self.composition.degree()
	}

	pub fn n_multilinears(&self) -> usize {
		self.composition.n_vars()
	}

	pub fn binary_tower_level(&self) -> usize {
		self.composition.binary_tower_level().max(
			self.inner
				.iter()
				.map(MultilinearPolyOracle::binary_tower_level)
				.max()
				.unwrap_or(0),
		)
	}

	pub const fn n_vars(&self) -> usize {
		self.n_vars
	}

	pub fn inner_polys_oracle_ids(&self) -> impl Iterator<Item = OracleId> + '_ {
		self.inner.iter().map(|oracle| oracle.id())
	}

	pub fn inner_polys(&self) -> Vec<MultilinearPolyOracle<F>> {
		self.inner.clone()
	}

	pub fn composition(&self) -> Arc<dyn CompositionPoly<F>> {
		self.composition.clone()
	}
}

#[cfg(test)]
mod tests {
	use binius_field::{BinaryField2b, BinaryField8b, BinaryField32b, BinaryField128b, TowerField};
	use binius_math::{ArithCircuit, ArithExpr};

	use super::*;
	use crate::oracle::MultilinearOracleSet;

	#[derive(Clone, Debug)]
	struct TestByteComposition;
	impl CompositionPoly<BinaryField128b> for TestByteComposition {
		fn n_vars(&self) -> usize {
			3
		}

		fn degree(&self) -> usize {
			1
		}

		fn expression(&self) -> ArithCircuit<BinaryField128b> {
			(ArithExpr::Var(0) * ArithExpr::Var(1)
				+ ArithExpr::Var(2) * ArithExpr::Const(BinaryField128b::new(125)))
			.into()
		}

		fn evaluate(
			&self,
			query: &[BinaryField128b],
		) -> Result<BinaryField128b, binius_math::Error> {
			Ok(query[0] * query[1] + query[2] * BinaryField128b::new(125))
		}

		fn binary_tower_level(&self) -> usize {
			BinaryField8b::TOWER_LEVEL
		}
	}

	#[test]
	fn test_composite_tower_level() {
		type F = BinaryField128b;

		let n_vars = 5;

		let mut oracles = MultilinearOracleSet::<F>::new();
		let poly_2b = oracles.add_committed(n_vars, BinaryField2b::TOWER_LEVEL);
		let poly_8b = oracles.add_committed(n_vars, BinaryField8b::TOWER_LEVEL);
		let poly_32b = oracles.add_committed(n_vars, BinaryField32b::TOWER_LEVEL);

		let composition = TestByteComposition;
		let composite = CompositePolyOracle::new(
			n_vars,
			vec![
				oracles[poly_2b].clone(),
				oracles[poly_2b].clone(),
				oracles[poly_2b].clone(),
			],
			composition.clone(),
		)
		.unwrap();
		assert_eq!(composite.binary_tower_level(), BinaryField8b::TOWER_LEVEL);

		let composite = CompositePolyOracle::new(
			n_vars,
			vec![
				oracles[poly_2b].clone(),
				oracles[poly_8b].clone(),
				oracles[poly_8b].clone(),
			],
			composition.clone(),
		)
		.unwrap();
		assert_eq!(composite.binary_tower_level(), BinaryField8b::TOWER_LEVEL);

		let composite = CompositePolyOracle::new(
			n_vars,
			vec![
				oracles[poly_2b].clone(),
				oracles[poly_8b].clone(),
				oracles[poly_32b].clone(),
			],
			composition,
		)
		.unwrap();
		assert_eq!(composite.binary_tower_level(), BinaryField32b::TOWER_LEVEL);
	}
}
