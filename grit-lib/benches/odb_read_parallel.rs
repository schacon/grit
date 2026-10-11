//! Parallel packed-object read throughput (Criterion).

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod fixture;

use std::hint::black_box;
use std::sync::OnceLock;
use std::sync::{Arc, Barrier};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use fixture::build_parallel_packed_blob_fixture;
use grit_lib::objects::ObjectId;
use grit_lib::odb::Odb;
use grit_lib::pack::clear_pack_cache;
use grit_lib::pack_store::PackStore;
use tempfile::TempDir;

struct ParallelReadFixture {
    _root: TempDir,
    odb: Odb,
    oids: Vec<ObjectId>,
}

static FIXTURE: OnceLock<ParallelReadFixture> = OnceLock::new();

fn fixture() -> &'static ParallelReadFixture {
    FIXTURE.get_or_init(|| {
        clear_pack_cache();
        let (root, odb, oids) = build_parallel_packed_blob_fixture(50_000);
        ParallelReadFixture {
            _root: root,
            odb,
            oids,
        }
    })
}

fn bench_parallel_reads(c: &mut Criterion) {
    let fx = fixture();
    let mut group = c.benchmark_group("odb_read_parallel/packed_blobs");
    group.throughput(Throughput::Elements(fx.oids.len() as u64));
    for threads in [1_usize, 2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::from_parameter(threads),
            &threads,
            |b, &thread_count| {
                b.iter(|| {
                    let odb = fx.odb.clone();
                    let store = Arc::clone(odb.pack_store().expect("pack store"));
                    let oids = fx.oids.clone();
                    let barrier = Arc::new(Barrier::new(thread_count));
                    std::thread::scope(|scope| {
                        let chunk = oids.len().div_ceil(thread_count);
                        for t in 0..thread_count {
                            let start = t * chunk;
                            if start >= oids.len() {
                                break;
                            }
                            let end = (start + chunk).min(oids.len());
                            let b = Arc::clone(&barrier);
                            let odb_t = odb.clone();
                            let pack_store = Arc::clone(&store);
                            let slice = oids[start..end].to_vec();
                            scope.spawn(move || {
                                PackStore::with_context(pack_store, || {
                                    b.wait();
                                    for oid in slice {
                                        black_box(odb_t.read(&oid).expect("read").data);
                                    }
                                });
                            });
                        }
                    });
                });
            },
        );
    }
    group.finish();
}

criterion_group!(odb_read_parallel, bench_parallel_reads);
criterion_main!(odb_read_parallel);
