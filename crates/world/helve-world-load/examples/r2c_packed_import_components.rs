use std::{cell::Cell, env, fs, hint::black_box, path::PathBuf, time::Instant};

use helve_benchmark_support::collect_hardware_metadata;
use helve_generated::{AIR, BlockStateId, GeneratedStateFacts};
use helve_world_contract::{BlockSection, SectionBlockPos};
use helve_world_import::{
    BlockProperty, BlockSectionDecodeScratch, BlockStateResolver, ChunkCompression,
    ChunkPayloadDecoder, ChunkPayloadLimits, DeflateChunkPayloadDecoder,
    ImportedBlockSectionBuilder, NbtLimits, RegionLimits, RegionView, StoredBlockImporter,
    Target262BlockStateResolver,
};
use helve_world_reference::DirectBlockSection;

const SECTOR_BYTES: usize = 4096;
const MAX_REGION_BYTES: usize = 16 * SECTOR_BYTES;
const MAX_INLINE_COMPRESSED_BYTES: usize = 4 * SECTOR_BYTES;
const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024;
const PACKED_LOCAL_X: u8 = 1;
const PACKED_LOCAL_Z: u8 = 0;

#[derive(Clone, Copy)]
enum Mode {
    Smoke,
    Full,
}

impl Mode {
    const fn rounds(self) -> usize {
        match self {
            Self::Smoke => 16,
            Self::Full => 256,
        }
    }

    const fn warmups(self) -> usize {
        match self {
            Self::Smoke => 4,
            Self::Full => 32,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Full => "full",
        }
    }
}

struct Config {
    mode: Mode,
    require_single_cpu: bool,
    packed_region: PathBuf,
}

#[derive(Clone, Copy, Default)]
struct ComponentMetrics {
    elapsed: u128,
    calls: usize,
}

fn record_metric(cell: &Cell<ComponentMetrics>, elapsed: u128) {
    let mut metrics = cell.get();
    metrics.elapsed = metrics.elapsed.saturating_add(elapsed);
    metrics.calls += 1;
    cell.set(metrics);
}

struct TimedDecoder<'a, D> {
    inner: &'a mut D,
    metrics: &'a Cell<ComponentMetrics>,
}

impl<D: ChunkPayloadDecoder> ChunkPayloadDecoder for TimedDecoder<'_, D> {
    type Error = D::Error;

    fn decode<'a>(
        &'a mut self,
        compression: ChunkCompression,
        payload: &'a [u8],
        max_decompressed_bytes: usize,
    ) -> Result<&'a [u8], Self::Error> {
        let Self { inner, metrics } = self;
        let start = Instant::now();
        let result = inner.decode(compression, payload, max_decompressed_bytes);
        record_metric(metrics, start.elapsed().as_nanos());
        result
    }
}

struct TimedResolver<'a, R> {
    inner: &'a R,
    metrics: &'a Cell<ComponentMetrics>,
}

impl<R: BlockStateResolver> BlockStateResolver for TimedResolver<'_, R> {
    type State = R::State;

    fn resolve(&self, name: &str, properties: &[BlockProperty<'_>]) -> Option<Self::State> {
        let start = Instant::now();
        let result = self.inner.resolve(name, properties);
        record_metric(self.metrics, start.elapsed().as_nanos());
        result
    }
}

#[derive(Default)]
struct DirectBuilder {
    metrics: ComponentMetrics,
}

impl DirectBuilder {
    fn reset_metrics(&mut self) {
        self.metrics = ComponentMetrics::default();
    }

    const fn metrics(&self) -> ComponentMetrics {
        self.metrics
    }

    fn record_build(&mut self, start: Instant) {
        self.metrics.elapsed = self
            .metrics
            .elapsed
            .saturating_add(start.elapsed().as_nanos());
        self.metrics.calls += 1;
    }
}

impl ImportedBlockSectionBuilder<BlockStateId> for DirectBuilder {
    type Section = DirectBlockSection<BlockStateId>;

    fn build_uniform(&mut self, state: BlockStateId) -> Self::Section {
        let start = Instant::now();
        let section = DirectBlockSection::filled(state, &GeneratedStateFacts);
        self.record_build(start);
        section
    }

    fn build_states(&mut self, states: &[BlockStateId]) -> Self::Section {
        let start = Instant::now();
        let first = states.first().copied().unwrap_or(AIR);
        let mut section = DirectBlockSection::filled(first, &GeneratedStateFacts);
        for y in 0_u8..16 {
            for z in 0_u8..16 {
                for x in 0_u8..16 {
                    let pos = SectionBlockPos::new(x, y, z).expect("bounded section coordinate");
                    let state = states[pos.index()];
                    if state != first {
                        section.replace(pos, state, &GeneratedStateFacts);
                    }
                }
            }
        }
        self.record_build(start);
        section
    }
}

#[derive(Clone, Copy)]
struct Sample {
    import: u128,
    payload_decode: u128,
    state_resolve: u128,
    section_build: u128,
    residual: u128,
    decode_calls: usize,
    resolve_calls: usize,
    build_calls: usize,
}

#[derive(Clone, Copy)]
struct Summary {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("r2c packed import component benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let hardware = collect_hardware_metadata()?;
    if config.require_single_cpu && hardware.single_allowed_cpu().is_none() {
        return Err(format!(
            "--require-single-cpu requested but affinity is {}",
            hardware.cpus_allowed_list
        ));
    }

    let region_bytes = fs::read(&config.packed_region).map_err(|error| {
        format!(
            "could not read {}: {error}",
            config.packed_region.display()
        )
    })?;
    if region_bytes.len() > MAX_REGION_BYTES {
        return Err(format!(
            "packed benchmark region exceeds {MAX_REGION_BYTES} bytes: {}",
            region_bytes.len()
        ));
    }
    let region = RegionView::new(
        &region_bytes,
        0,
        0,
        RegionLimits::new(MAX_REGION_BYTES, MAX_INLINE_COMPRESSED_BYTES),
    )
    .map_err(|error| format!("packed benchmark region rejected: {error:?}"))?;
    let nbt_limits = NbtLimits::new(1024, 1024, 8192, 32)
        .map_err(|error| format!("invalid NBT limits: {error:?}"))?;
    let payload_limits =
        ChunkPayloadLimits::new(MAX_INLINE_COMPRESSED_BYTES, MAX_DECOMPRESSED_BYTES);
    let mut decoder = DeflateChunkPayloadDecoder::try_with_output_limit(MAX_DECOMPRESSED_BYTES)
        .map_err(|error| format!("decoder init failed: {error:?}"))?;
    let mut builder = DirectBuilder::default();
    let mut scratch = BlockSectionDecodeScratch::new();

    for _ in 0..config.mode.warmups() {
        black_box(run_sample(
            &region,
            payload_limits,
            nbt_limits,
            &mut decoder,
            &mut builder,
            &mut scratch,
        )?);
    }

    let mut samples = Vec::with_capacity(config.mode.rounds());
    for _ in 0..config.mode.rounds() {
        samples.push(run_sample(
            &region,
            payload_limits,
            nbt_limits,
            &mut decoder,
            &mut builder,
            &mut scratch,
        )?);
    }

    let first = samples
        .first()
        .copied()
        .ok_or_else(|| "benchmark produced no samples".to_owned())?;
    for sample in &samples {
        if sample.decode_calls != first.decode_calls
            || sample.resolve_calls != first.resolve_calls
            || sample.build_calls != first.build_calls
        {
            return Err("component call counts changed across benchmark rounds".to_owned());
        }
    }

    let import = summarize(samples.iter().map(|sample| sample.import).collect());
    let payload_decode = summarize(
        samples
            .iter()
            .map(|sample| sample.payload_decode)
            .collect(),
    );
    let state_resolve = summarize(
        samples
            .iter()
            .map(|sample| sample.state_resolve)
            .collect(),
    );
    let section_build = summarize(
        samples
            .iter()
            .map(|sample| sample.section_build)
            .collect(),
    );
    let residual = summarize(samples.iter().map(|sample| sample.residual).collect());

    println!(
        "{{\"schema\":1,\"kind\":\"r2c-packed-import-components\",\"mode\":\"{}\",\"fixture\":\"differential-packed4-gzip\",\"diagnostic_only\":true,\"performance_admitted\":false,\"rounds\":{},\"decode_calls\":{},\"resolve_calls\":{},\"section_build_calls\":{},\"import_ns\":{},\"payload_decode_ns\":{},\"state_resolve_ns\":{},\"section_build_ns\":{},\"residual_ns\":{},\"hardware\":{}}}",
        config.mode.as_str(),
        samples.len(),
        first.decode_calls,
        first.resolve_calls,
        first.build_calls,
        summary_json(import),
        summary_json(payload_decode),
        summary_json(state_resolve),
        summary_json(section_build),
        summary_json(residual),
        hardware.to_json(),
    );
    Ok(())
}

fn run_sample(
    region: &RegionView<'_>,
    payload_limits: ChunkPayloadLimits,
    nbt_limits: NbtLimits,
    decoder: &mut DeflateChunkPayloadDecoder,
    builder: &mut DirectBuilder,
    scratch: &mut BlockSectionDecodeScratch<BlockStateId>,
) -> Result<Sample, String> {
    let decode_metrics = Cell::new(ComponentMetrics::default());
    let resolve_metrics = Cell::new(ComponentMetrics::default());
    let resolver = Target262BlockStateResolver;
    let timed_resolver = TimedResolver {
        inner: &resolver,
        metrics: &resolve_metrics,
    };
    let mut timed_decoder = TimedDecoder {
        inner: decoder,
        metrics: &decode_metrics,
    };
    builder.reset_metrics();

    let import_start = Instant::now();
    let stored_chunk = {
        let mut importer = StoredBlockImporter::new(
            payload_limits,
            nbt_limits,
            &mut timed_decoder,
            &timed_resolver,
            builder,
            scratch,
        );
        importer
            .import_region_chunk(region, PACKED_LOCAL_X, PACKED_LOCAL_Z, None)
            .map_err(|error| format!("packed import failed: {error:?}"))?
    };
    let import = import_start.elapsed().as_nanos();
    black_box(&stored_chunk.blocks);
    if stored_chunk.blocks.sections.len() != 1 {
        return Err(format!(
            "packed fixture produced {} block sections instead of 1",
            stored_chunk.blocks.sections.len()
        ));
    }

    let decode = decode_metrics.get();
    let resolve = resolve_metrics.get();
    let build = builder.metrics();
    if decode.calls != 1 {
        return Err(format!(
            "packed importer invoked payload decoder {} times instead of 1",
            decode.calls
        ));
    }
    if resolve.calls == 0 {
        return Err("packed importer did not invoke block-state resolver".to_owned());
    }
    if build.calls == 0 {
        return Err("packed importer did not invoke section builder".to_owned());
    }

    let measured = decode
        .elapsed
        .saturating_add(resolve.elapsed)
        .saturating_add(build.elapsed);
    let residual = import.checked_sub(measured).ok_or_else(|| {
        format!(
            "component timings exceed import total: import={import} components={measured}"
        )
    })?;

    Ok(Sample {
        import,
        payload_decode: decode.elapsed,
        state_resolve: resolve.elapsed,
        section_build: build.elapsed,
        residual,
        decode_calls: decode.calls,
        resolve_calls: resolve.calls,
        build_calls: build.calls,
    })
}

fn parse_args() -> Result<Config, String> {
    let mut mode = None;
    let mut require_single_cpu = false;
    let mut packed_region = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--require-single-cpu" => require_single_cpu = true,
            "--packed4-region" => {
                if packed_region.is_some() {
                    return Err("--packed4-region may be specified only once".to_owned());
                }
                let path = args
                    .next()
                    .ok_or_else(|| "--packed4-region requires a path".to_owned())?;
                packed_region = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                return Err(
                    "usage: r2c_packed_import_components (--smoke|--full) [--require-single-cpu] --packed4-region PATH"
                        .to_owned(),
                );
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(Config {
        mode: mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?,
        require_single_cpu,
        packed_region: packed_region
            .ok_or_else(|| "--packed4-region PATH is required".to_owned())?,
    })
}

fn set_mode(slot: &mut Option<Mode>, mode: Mode) -> Result<(), String> {
    if slot.replace(mode).is_some() {
        return Err("specify exactly one benchmark mode".to_owned());
    }
    Ok(())
}

fn summarize(mut values: Vec<u128>) -> Summary {
    values.sort_unstable();
    Summary {
        p50: percentile(&values, 50),
        p95: percentile(&values, 95),
        p99: percentile(&values, 99),
        max: values.last().copied().unwrap_or_default(),
    }
}

fn percentile(values: &[u128], percentile: usize) -> u128 {
    if values.is_empty() {
        return 0;
    }
    let index = (values.len() - 1) * percentile / 100;
    values[index]
}

fn summary_json(summary: Summary) -> String {
    format!(
        "{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}}",
        summary.p50, summary.p95, summary.p99, summary.max
    )
}
