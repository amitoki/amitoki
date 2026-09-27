use serde::Serialize;
use std::time::Duration;

/// log2マイクロ秒の固定長ヒストグラム。試験時間によらずメモリ使用量を固定する。
#[derive(Default)]
pub(super) struct Latencies {
    buckets: [u64; 32],
    count: u64,
}

#[derive(Serialize)]
pub(super) struct Percentiles {
    pub p50_upper_bound_us: u64,
    pub p95_upper_bound_us: u64,
    pub p99_upper_bound_us: u64,
}

impl Latencies {
    pub fn record(&mut self, duration: Duration) {
        // 小数部を切り捨てると、実測値より小さい「上限」を報告してしまう。
        let microseconds = duration.as_nanos().div_ceil(1000).max(1).min(u32::MAX as u128) as u32;
        let bucket = microseconds.checked_next_power_of_two().map_or(31, |value| value.trailing_zeros() as usize);
        self.buckets[bucket] += 1;
        self.count += 1;
    }

    fn percentile(&self, percent: u64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let target = (self.count * percent).div_ceil(100);
        let mut count = 0;
        for (index, samples) in self.buckets.iter().enumerate() {
            count += samples;
            if count >= target {
                return if index == 31 { u32::MAX as u64 } else { 1 << index };
            }
        }
        0
    }

    pub fn percentiles(&self) -> Percentiles {
        Percentiles {
            p50_upper_bound_us: self.percentile(50),
            p95_upper_bound_us: self.percentile(95),
            p99_upper_bound_us: self.percentile(99),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_upper_bounds_include_fractional_microseconds() {
        let mut latencies = Latencies::default();
        latencies.record(Duration::from_nanos(1500));
        assert_eq!(latencies.percentiles().p50_upper_bound_us, 2);
        latencies.record(Duration::from_nanos(2001));
        assert_eq!(latencies.percentiles().p95_upper_bound_us, 4);
    }
}
