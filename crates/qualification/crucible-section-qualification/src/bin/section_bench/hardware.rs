pub(crate) use crucible_benchmark_support::HardwareMetadata;
use crucible_benchmark_support::collect_hardware_metadata;

pub(crate) fn collect() -> Result<HardwareMetadata, String> {
    collect_hardware_metadata()
}
