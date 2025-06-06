// Copyright 2024-2025 Irreducible Inc.

use std::iter::repeat_with;

use binius_core::{
	fiat_shamir::HasherChallenger,
	protocols::gkr_gpa::{self, GrandProductClaim, GrandProductWitness},
	transcript::ProverTranscript,
};
use binius_field::{
	AESTowerField8b, AESTowerField128b, BINARY_TO_POLYVAL_TRANSFORMATION, BinaryField,
	BinaryField8b, BinaryField128b, BinaryField128bPolyval, PackedExtension, PackedField,
	PackedFieldIndexable, TowerField,
	arch::{OptimalUnderlier, OptimalUnderlierByteSliced},
	as_packed_field::{PackScalar, PackedType},
	linear_transformation::{PackedTransformationFactory, Transformation},
};
use binius_hal::{CpuBackend, make_portable_backend};
use binius_hash::groestl::Groestl256;
use binius_math::{EvaluationOrder, IsomorphicEvaluationDomainFactory};
use binius_maybe_rayon::iter::{IntoParallelIterator, ParallelIterator};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rand::{SeedableRng, rngs::StdRng};

// Creates T(x), a multilinear with evaluations over the n-dimensional boolean hypercube
fn create_numerator<P: PackedField>(n_vars: usize) -> Vec<P> {
	let mut rng = StdRng::seed_from_u64(0);
	repeat_with(|| P::random(&mut rng))
		.take(1 << (n_vars - P::LOG_WIDTH))
		.collect()
}

fn apply_transformation<IP, OP>(
	input: &[IP],
	transformation: &impl Transformation<IP, OP>,
) -> Vec<OP> {
	input.iter().map(|x| transformation.transform(x)).collect()
}

const N_VARS: [usize; 4] = [12, 16, 20, 24];

const N_CLAIMS: usize = 1;
type FDomain = BinaryField128b;

fn bench_gpa_generic<P, FDomain, R, BenchFn>(name: &str, c: &mut Criterion, bench_fn: &BenchFn)
where
	P: PackedField<Scalar: TowerField + From<BinaryField128b>> + PackedExtension<FDomain>,
	FDomain: BinaryField,
	BenchFn: Fn(
		usize,
		&mut ProverTranscript<HasherChallenger<Groestl256>>,
		&[P],
		&IsomorphicEvaluationDomainFactory<FDomain>,
		&CpuBackend,
	) -> R,
{
	let mut group = c.benchmark_group(name);
	let domain_factory = IsomorphicEvaluationDomainFactory::<FDomain>::default();

	for n_vars in N_VARS {
		group.throughput(Throughput::Elements(((1 << n_vars) * N_CLAIMS) as u64));
		group.sample_size(10);
		group.bench_function(format!("n_vars={n_vars}"), |bench| {
			// Setup witness
			let numerator = create_numerator::<P>(n_vars);

			let backend = make_portable_backend();

			let mut prover_transcript = ProverTranscript::<HasherChallenger<Groestl256>>::default();
			bench.iter(|| {
				bench_fn(n_vars, &mut prover_transcript, &numerator, &domain_factory, &backend)
			});
		});
	}
	group.finish()
}

fn bench_gpa<P, FDomain>(name: &str, evaluation_order: EvaluationOrder, c: &mut Criterion)
where
	P: PackedField<Scalar: TowerField + From<BinaryField128b>> + PackedExtension<FDomain>,
	FDomain: BinaryField,
{
	bench_gpa_generic::<P, FDomain, _, _>(
		name,
		c,
		&|n_vars, prover_transcript, numerator, domain_factory, backend| {
			let (gpa_witnesses, gpa_claims): (Vec<_>, Vec<_>) = (0..N_CLAIMS)
				.into_par_iter()
				.map(|_| {
					let gpa_witness =
						GrandProductWitness::<P>::new(n_vars, numerator.to_vec()).unwrap();
					let product = gpa_witness.grand_product_evaluation();
					(gpa_witness, GrandProductClaim { n_vars, product })
				})
				.collect::<Vec<_>>()
				.into_iter()
				.unzip();

			gkr_gpa::batch_prove::<P::Scalar, P, FDomain, _, _>(
				evaluation_order,
				gpa_witnesses,
				&gpa_claims,
				domain_factory.clone(),
				prover_transcript,
				backend,
			)
		},
	);
}

fn bench_gpa_polyval_with_isomorphism<U>(
	name: &str,
	evaluation_order: EvaluationOrder,
	c: &mut Criterion,
) where
	U: PackScalar<
			BinaryField128b,
			Packed: PackedFieldIndexable
			            + PackedTransformationFactory<
				<U as PackScalar<BinaryField128bPolyval>>::Packed,
			>,
		> + PackScalar<
			BinaryField128bPolyval,
			Packed: PackedFieldIndexable
			            + PackedTransformationFactory<<U as PackScalar<BinaryField128b>>::Packed>,
		>,
{
	let transform_to_polyval =
		<U as PackScalar<BinaryField128b>>::Packed::make_packed_transformation(
			BINARY_TO_POLYVAL_TRANSFORMATION,
		);

	bench_gpa_generic::<PackedType<U, BinaryField128b>, FDomain, _, _>(
		name,
		c,
		&|n_vars, prover_transcript, numerator, domain_factory, backend| {
			let (gpa_witnesses, gpa_claims): (Vec<_>, Vec<_>) = (0..N_CLAIMS)
				.into_par_iter()
				.map(|_| {
					let transformed_values = apply_transformation(numerator, &transform_to_polyval);
					let gpa_witness = GrandProductWitness::new(n_vars, transformed_values).unwrap();
					let product = gpa_witness.grand_product_evaluation();
					(gpa_witness, GrandProductClaim { n_vars, product })
				})
				.collect::<Vec<_>>()
				.into_iter()
				.unzip();

			gkr_gpa::batch_prove::<
				BinaryField128bPolyval,
				<U as PackScalar<BinaryField128bPolyval>>::Packed,
				BinaryField128bPolyval,
				_,
				_,
			>(
				evaluation_order,
				gpa_witnesses,
				&gpa_claims,
				domain_factory.clone(),
				prover_transcript,
				&backend,
			)
		},
	);
}

fn bench_polyval(c: &mut Criterion) {
	bench_gpa::<PackedType<OptimalUnderlier, BinaryField128bPolyval>, BinaryField128bPolyval>(
		"gpa_polyval_128b",
		EvaluationOrder::LowToHigh,
		c,
	);
}

fn bench_polyval_high_to_low(c: &mut Criterion) {
	bench_gpa::<PackedType<OptimalUnderlier, BinaryField128bPolyval>, BinaryField128bPolyval>(
		"gpa_polyval_128b_high_to_low",
		EvaluationOrder::HighToLow,
		c,
	);
}

fn bench_binary_128b(c: &mut Criterion) {
	bench_gpa::<PackedType<OptimalUnderlier, BinaryField128b>, BinaryField8b>(
		"gpa_binary_128b",
		EvaluationOrder::LowToHigh,
		c,
	);
}

fn bench_byte_sliced_aes_128b(c: &mut Criterion) {
	bench_gpa::<PackedType<OptimalUnderlierByteSliced, AESTowerField128b>, AESTowerField8b>(
		"gpa_byte_sliced_aes_128b",
		EvaluationOrder::HighToLow,
		c,
	);
}

fn bench_binary_128b_isomorphic(c: &mut Criterion) {
	bench_gpa_polyval_with_isomorphism::<OptimalUnderlier>(
		"gpa_binary_128b_isomorphic",
		EvaluationOrder::LowToHigh,
		c,
	);
}

criterion_main!(prodcheck);
criterion_group!(
	prodcheck,
	bench_polyval,
	bench_polyval_high_to_low,
	bench_binary_128b,
	bench_byte_sliced_aes_128b,
	bench_binary_128b_isomorphic
);
