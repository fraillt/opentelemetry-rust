/*
    Stress test results:
    OS: Ubuntu 22.04.4 LTS (5.15.153.1-microsoft-standard-WSL2)
    Hardware: Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz, 16vCPUs,
    RAM: 64.0 GB
    ~11.5 M/sec
*/

use lazy_static::lazy_static;
use opentelemetry::{
    global,
    metrics::{noop::NoopMeterProvider, Gauge, MeterProvider as _},
    KeyValue,
};
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};
use rand::{
    rngs::{self},
    Rng, SeedableRng,
};
use std::cell::RefCell;
use throughput::ThroughputTest;

mod throughput;

lazy_static! {
    // static ref PROVIDER: SdkMeterProvider = SdkMeterProvider::builder()
    //     .with_reader(ManualReader::builder().build())
    //     .build();
    // static ref ATTRIBUTE_VALUES: [&'static str; 10] = [
    //     "value1", "value2", "value3", "value4", "value5", "value6", "value7", "value8", "value9",
    //     "value10"
    // ];
    // static ref GAUGE: Gauge<u64> = PROVIDER.meter("test").u64_gauge("test_gauge").init();
}

thread_local! {
    /// Store random number generator for each thread
    static CURRENT_RNG: RefCell<rngs::SmallRng> = RefCell::new(rngs::SmallRng::from_entropy());
}

struct Test {
    gauges: Vec<Gauge<u64>>,
    values: [&'static str; 10],
}

impl ThroughputTest for Test {
    fn run(&self) {
        let len = self.values.len();
        let rands = CURRENT_RNG.with(|rng| {
            let mut rng = rng.borrow_mut();
            [
                rng.gen_range(0..len),
                rng.gen_range(0..len),
                rng.gen_range(0..len),
            ]
        });
        let index_first_attribute = rands[0];
        let index_second_attribute = rands[1];
        let index_third_attribute = rands[2];

        // each attribute has 10 possible values, so there are 1000 possible combinations (time-series)
        self.gauges[0].record(
            1,
            &[
                KeyValue::new("attribute1", self.values[index_first_attribute]),
                KeyValue::new("attribute2", self.values[index_second_attribute]),
                KeyValue::new("attribute3", self.values[index_third_attribute]),
            ],
        );
    }
}

fn main() {
    // when I set global meter, things slowsdown (unless specific conditions, see comment below)
    global::set_meter_provider(
        SdkMeterProvider::builder()
            .with_reader(ManualReader::builder().build())
            .build(),
    );

    let provider = SdkMeterProvider::builder()
        .with_reader(ManualReader::builder().build())
        .build();
    let values = [
        "value1", "value2", "value3", "value4", "value5", "value6", "value7", "value8",
        "value9", "value10",
    ];

    let test = Test {
        // even if global meter is set: 1 - fast, 5 - fast, others are slow (not tested all)
        gauges: (0..5)
            .into_iter()
            .map(|i| {
                provider
                    .meter(values[i])
                    .u64_gauge("test_gauge")
                    .init()
            })
            .collect(),
        values,
    };

    throughput::test_throughput(test);
}
