//! Criterion benchmarks for pack-objects on the ~100k-object repacked fixture:
//! on-disk delta/object reuse vs fresh windowed delta search.

#![allow(clippy::expect_used, clippy::unwrap_used, dead_code)]

mod fixture;

use std::hint::black_box;
use std::path::Path;
use std::sync::OnceLock;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use fixture::OdbBackendFixtures;
use grit_lib::objects::pack::read_pack_index;
use grit_lib::objects::pack_objects::{build_pack_with_parallel_threads, PackBuildOptions};

struct PackWriteBench {
    object_count: u64,
    reuse_opts: PackBuildOptions,
    fresh_opts: PackBuildOptions,
}

fn pack_write_bench() -> &'static PackWriteBench {
    static BENCH: OnceLock<PackWriteBench> = OnceLock::new();
    BENCH.get_or_init(|| {
        let fx = OdbBackendFixtures::global();
        let case = &fx.pack_100k;
        let object_count = pack_object_count(case.odb.objects_dir()) as u64;
        let reuse_opts = PackBuildOptions {
            use_bitmaps: false,
            ..PackBuildOptions::for_local_copy(None)
        };
        let fresh_opts = PackBuildOptions {
            delta: true,
            reuse_objects: false,
            reuse_deltas: false,
            use_bitmaps: false,
            ..PackBuildOptions::default()
        };
        PackWriteBench {
            object_count,
            reuse_opts,
            fresh_opts,
        }
    })
}

fn pack_object_count(objects_dir: &Path) -> usize {
    let pack_dir = objects_dir.join("pack");
    let idx_path = std::fs::read_dir(&pack_dir)
        .expect("pack dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "idx"))
        .expect("pack index in 100k fixture");
    read_pack_index(&idx_path).expect("read pack idx").len()
}

fn bench_pack_write_100k(c: &mut Criterion) {
    let fx = OdbBackendFixtures::global();
    let case = &fx.pack_100k;
    let tip = case.hit;
    let meta = pack_write_bench();

    let mut group = c.benchmark_group("pack");
    group.throughput(Throughput::Elements(meta.object_count));

    group.bench_function("write_100k_reuse", |b| {
        b.iter(|| {
            black_box(
                build_pack_with_parallel_threads(
                    &case.odb,
                    &[tip],
                    &[],
                    &meta.reuse_opts,
                    None,
                    None,
                )
                .expect("reuse pack"),
            )
        });
    });

    group.bench_function("write_100k_fresh", |b| {
        b.iter(|| {
            black_box(
                build_pack_with_parallel_threads(
                    &case.odb,
                    &[tip],
                    &[],
                    &meta.fresh_opts,
                    None,
                    None,
                )
                .expect("fresh pack"),
            )
        });
    });

    group.finish();
}

criterion_group!(pack_write, bench_pack_write_100k);
criterion_main!(pack_write);
