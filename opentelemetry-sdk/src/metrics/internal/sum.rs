use std::mem::replace;
use std::ops::DerefMut;
use std::vec;
use std::{sync::Mutex, time::SystemTime};

use crate::metrics::data::{self, DataPoint};
use crate::metrics::Temporality;
use opentelemetry::KeyValue;

use super::aggregate::InitAggregationData;
use super::{Aggregator, AtomicTracker, Number};
use super::{AtomicallyUpdate, ValueMap};

struct Increment<T>
where
    T: AtomicallyUpdate<T>,
{
    value: T::AtomicTracker,
}

impl<T> Aggregator for Increment<T>
where
    T: Number,
{
    type InitConfig = ();
    type PreComputedValue = T;

    fn create(_init: &()) -> Self {
        Self {
            value: T::new_atomic_tracker(T::default()),
        }
    }

    fn update(&self, value: T) {
        self.value.add(value)
    }

    fn clone_and_reset(&self, _: &()) -> Self {
        Self {
            value: T::new_atomic_tracker(self.value.get_and_reset_value()),
        }
    }
}

/// Summarizes a set of measurements made as their arithmetic sum.
pub(crate) struct Sum<T: Number> {
    value_map: ValueMap<Increment<T>>,
    monotonic: bool,
    start: Mutex<SystemTime>,
}

impl<T: Number> Sum<T> {
    /// Returns an aggregator that summarizes a set of measurements as their
    /// arithmetic sum.
    ///
    /// Each sum is scoped by attributes and the aggregation cycle the measurements
    /// were made in.
    pub(crate) fn new(monotonic: bool) -> Self {
        Sum {
            value_map: ValueMap::new(()),
            monotonic,
            start: Mutex::new(SystemTime::now()),
        }
    }

    pub(crate) fn measure(&self, measurement: T, attrs: &[KeyValue]) {
        // The argument index is not applicable to Sum.
        self.value_map.measure(measurement, attrs);
    }

    pub(crate) fn delta(
        &self,
        data_points: &mut Vec<data::DataPoint<T>>,
    ) {
        let t = SystemTime::now();

        let prev_start = self
            .start
            .lock()
            .map(|mut start| replace(start.deref_mut(), t))
            .unwrap_or(t);
        self.value_map
            .collect_and_reset(data_points, |attributes, aggr| DataPoint {
                attributes,
                start_time: prev_start,
                time: t,
                value: aggr.value.get_value(),
                exemplars: vec![],
            });
    }

    pub(crate) fn cumulative(
        &self,
        data_points: &mut Vec<data::DataPoint<T>>,
    ) {

        let t = SystemTime::now();
        let prev_start = self.start.lock().map(|start| *start).unwrap_or(t);

        self.value_map
            .collect_readonly(data_points, |attributes, aggr| DataPoint {
                attributes,
                start_time: prev_start,
                time: t,
                value: aggr.value.get_value(),
                exemplars: vec![],
            });
    }
}

impl<T> InitAggregationData for Sum<T>
where
    T: Number,
{
    type Aggr = data::Sum<T>;

    fn create_new(&self, temporality: Temporality) -> Self::Aggr {
        data::Sum {
            data_points: vec![],
            temporality,
            is_monotonic: self.monotonic,
        }
    }

    fn init_existing(&self, existing: &mut Self::Aggr, temporality: Temporality) {
        existing.is_monotonic = self.monotonic;
        existing.temporality = temporality;
        existing.data_points.clear();
    }
}
