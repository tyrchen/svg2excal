//! End-to-end conversion and target-validation regression benchmarks.

use std::{fmt::Write as _, hint::black_box};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use svg2excal_core::{ConversionLimits, ConversionOptions, ExcalidrawDocument, convert};

const TINY: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="32"><rect x="2" y="2" width="60" height="28" fill="#74c0fc" stroke="#1971c2"/></svg>"##;
const RFC: &[u8] = include_bytes!("../fixtures/rfc.svg");
const FALLBACK: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="2000" height="2000"><defs><linearGradient id="g"><stop stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs><rect width="2000" height="2000" fill="url(#g)"/></svg>"##;

fn generated_nodes(count: usize, padding_bytes: usize) -> Vec<u8> {
    let mut svg =
        String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="1000">"#);
    let padding = "x".repeat(padding_bytes);
    for index in 0..count {
        let x = index % 100;
        let y = index / 100;
        if write!(
            &mut svg,
            r##"<rect x="{x}" y="{y}" width="1" height="1" fill="#2563eb" data-padding="{padding}"/>"##
        )
        .is_err()
        {
            std::process::abort();
        }
    }
    svg.push_str("</svg>");
    svg.into_bytes()
}

fn generated_paths(count: usize) -> Vec<u8> {
    let mut svg =
        String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="1000">"#);
    for index in 0..count {
        let y = index % 1000;
        if write!(
            &mut svg,
            r##"<path d="M0 {y} C250 {} 750 {} 1000 {y}" fill="none" stroke="#2563eb"/>"##,
            y.saturating_add(10),
            y.saturating_sub(10),
        )
        .is_err()
        {
            std::process::abort();
        }
    }
    svg.push_str("</svg>");
    svg.into_bytes()
}

fn generated_text(count: usize) -> Vec<u8> {
    let mut svg =
        String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="1000">"#);
    for index in 0..count {
        let y = index % 1000;
        if write!(
            &mut svg,
            r#"<text x="1" y="{y}" font-family="Liberation Sans" font-size="12">bounded text</text>"#,
        )
        .is_err()
        {
            std::process::abort();
        }
    }
    svg.push_str("</svg>");
    svg.into_bytes()
}

fn benchmark_conversion(criterion: &mut Criterion) {
    let options = ConversionOptions::default();
    criterion.bench_function("convert/tiny", |bencher| {
        bencher.iter(|| required_conversion(black_box(TINY), black_box(&options)));
    });
    criterion.bench_function("convert/rfc", |bencher| {
        bencher.iter(|| required_conversion(black_box(RFC), black_box(&options)));
    });
    criterion.bench_function("convert/5000-solid-nodes", |bencher| {
        bencher.iter_batched(
            || generated_nodes(5_000, 140),
            |input| required_conversion(black_box(&input), black_box(&options)),
            BatchSize::LargeInput,
        );
    });
    criterion.bench_function("convert/50000-solid-nodes-10mb", |bencher| {
        bencher.iter_batched(
            || generated_nodes(50_000, 140),
            |input| required_conversion(black_box(&input), black_box(&options)),
            BatchSize::LargeInput,
        );
    });
    criterion.bench_function("convert/1000-curved-paths", |bencher| {
        bencher.iter_batched(
            || generated_paths(1_000),
            |input| required_conversion(black_box(&input), black_box(&options)),
            BatchSize::LargeInput,
        );
    });
    criterion.bench_function("convert/1000-text-nodes", |bencher| {
        bencher.iter_batched(
            || generated_text(1_000),
            |input| required_conversion(black_box(&input), black_box(&options)),
            BatchSize::LargeInput,
        );
    });
    criterion.bench_function("convert/16-megapixel-fallback", |bencher| {
        bencher.iter(|| required_conversion(black_box(FALLBACK), black_box(&options)));
    });
    criterion.bench_function("convert/deterministic-rerun", |bencher| {
        let baseline = required_conversion(RFC, &options);
        let baseline_json = required_json(&baseline.document, &options.limits);
        bencher.iter(|| {
            let rerun = required_conversion(black_box(RFC), black_box(&options));
            if required_json(&rerun.document, &options.limits) != baseline_json {
                std::process::abort();
            }
        });
    });
}

fn benchmark_target_validation(criterion: &mut Criterion) {
    let options = ConversionOptions::default();
    let result = required_conversion(RFC, &options);
    let json = required_json(&result.document, &options.limits);
    criterion.bench_function("target/deserialize-validate", |bencher| {
        bencher.iter(|| {
            required_document_from_json(
                black_box(json.as_bytes()),
                black_box(&ConversionLimits::default()),
            )
        });
    });
    criterion.bench_function("target/serialize", |bencher| {
        bencher.iter(|| required_json(&result.document, black_box(&options.limits)));
    });
}

fn required_conversion(
    input: &[u8],
    options: &ConversionOptions,
) -> svg2excal_core::ConversionResult {
    match convert(input, options) {
        Ok(result) => result,
        Err(_) => std::process::abort(),
    }
}

fn required_json(document: &ExcalidrawDocument, limits: &ConversionLimits) -> String {
    match document.to_pretty_json_with_limits(limits) {
        Ok(json) => json,
        Err(_) => std::process::abort(),
    }
}

fn required_document_from_json(bytes: &[u8], limits: &ConversionLimits) -> ExcalidrawDocument {
    match ExcalidrawDocument::from_json(bytes, limits) {
        Ok(document) => document,
        Err(_) => std::process::abort(),
    }
}

criterion_group!(benches, benchmark_conversion, benchmark_target_validation);
criterion_main!(benches);
