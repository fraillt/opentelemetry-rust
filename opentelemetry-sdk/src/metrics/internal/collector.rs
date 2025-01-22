use std::sync::Arc;

use opentelemetry::KeyValue;

use crate::metrics::{
    data::{Aggregation, AggregationDataPoints},
    Temporality,
};

use super::{
    aggregate::{AggregateTime, AttributeSetFilter},
    AggregateFns, AggregateTimeInitiator, Aggregator, ComputeAggregation, Measure, Number,
    ValueMap,
};

/// Aggregate measurements for attribute sets and collect these aggregates into data points for specific temporality
pub(crate) trait AggregateMap: Send + Sync + 'static {
    const TEMPORALITY: Temporality;
    type Aggr: Aggregator;

    fn measure(&self, value: <Self::Aggr as Aggregator>::PreComputedValue, attributes: &[KeyValue]);

    fn collect_data_points<DP, MapFn>(&self, dest: &mut Vec<DP>, map_fn: MapFn)
    where
        MapFn: FnMut(Vec<KeyValue>, &Self::Aggr) -> DP;
}

pub(crate) trait AggregationMeasure<T>: Send + Sync + 'static {
    type Aggr: Aggregator;
    fn precompute(&self, value: T) -> <Self::Aggr as Aggregator>::PreComputedValue;
}

pub(crate) trait AggregationCollect: Send + Sync + 'static {
    type Aggr2: Aggregator;
    type Aggr: Aggregation + AggregationDataPoints;
    fn create_new(&self, temporality: Temporality, time: AggregateTime) -> Self::Aggr;
    fn reset_existing(
        &self,
        existing: &mut Self::Aggr,
        temporality: Temporality,
        time: AggregateTime,
    );
    fn build_create_points_fn(
        &self,
    ) -> impl FnMut(Vec<KeyValue>, &Self::Aggr2) -> <Self::Aggr as AggregationDataPoints>::DataPoint;
}

pub(crate) fn create_aggregation<A, AM, T>(
    aggregation: A,
    aggregate_map: AM,
    filter: AttributeSetFilter,
) -> AggregateFns<T>
where
    AM: AggregateMap,
    A: AggregationCollect<Aggr2 = AM::Aggr>,
    A: AggregationMeasure<T, Aggr = AM::Aggr>,
    T: Number,
{
    let collector = Arc::new(Collector::new(filter, aggregation, aggregate_map));
    AggregateFns {
        collect: collector.clone(),
        measure: collector,
    }
}

struct Collector<A, AM> {
    filter: AttributeSetFilter,
    aggregation: A,
    aggregate_map: AM,
    time: AggregateTimeInitiator,
}

impl<A, AM> Collector<A, AM>
where
    AM: AggregateMap,
{
    pub(crate) fn new(filter: AttributeSetFilter, aggregation: A, aggregate_map: AM) -> Self {
        Self {
            filter,
            aggregation,
            aggregate_map,
            time: AggregateTimeInitiator::default(),
        }
    }

    fn init_time(&self) -> AggregateTime {
        if let Temporality::Delta = AM::TEMPORALITY {
            self.time.delta()
        } else {
            self.time.cumulative()
        }
    }
}

impl<A, AM, T> Measure<T> for Collector<A, AM>
where
    A: AggregationMeasure<T>,
    AM: AggregateMap<Aggr = A::Aggr>,
    T: Number,
{
    fn call(&self, measurement: T, attrs: &[KeyValue]) {
        let precomputed = self.aggregation.precompute(measurement);
        self.filter.apply(attrs, |filtered_attrs| {
            self.aggregate_map.measure(precomputed, filtered_attrs);
        });
    }
}

impl<A, AM> ComputeAggregation for Collector<A, AM>
where
    A: AggregationCollect,
    AM: AggregateMap<Aggr = A::Aggr2>,
{
    fn call(&self, dest: Option<&mut dyn Aggregation>) -> (usize, Option<Box<dyn Aggregation>>) {
        let time = self.init_time();
        let mut s_data = dest.and_then(|d| d.as_mut().downcast_mut::<A::Aggr>());
        let mut new_agg = match s_data.as_mut() {
            Some(existing) => {
                self.aggregation
                    .reset_existing(existing, AM::TEMPORALITY, time);
                None
            }
            None => Some(self.aggregation.create_new(AM::TEMPORALITY, time)),
        };
        let s_data = s_data.unwrap_or_else(|| new_agg.as_mut().expect("present if s_data is none"));

        let mut create_point = self.aggregation.build_create_points_fn();
        self.aggregate_map
            .collect_data_points(s_data.points(), move |a, b| create_point(a, b));

        (
            s_data.points().len(),
            new_agg.map(|a| Box::new(a) as Box<_>),
        )
    }
}

/// At the moment use [`ValueMap`] under the hood (which support both Delta and Cumulative), to implement `AggregateMap` for Delta temporality
/// Later this could be improved to support only Delta temporality
pub(crate) struct DeltaValueMap<A>(ValueMap<A>)
where
    A: Aggregator;

impl<A> DeltaValueMap<A>
where
    A: Aggregator,
{
    pub(crate) fn new(config: A::InitConfig) -> Self {
        Self(ValueMap::new(config))
    }
}

impl<A> AggregateMap for DeltaValueMap<A>
where
    A: Aggregator,
    <A as Aggregator>::InitConfig: Send + Sync,
{
    const TEMPORALITY: Temporality = Temporality::Delta;

    type Aggr = A;

    fn measure(
        &self,
        value: <Self::Aggr as Aggregator>::PreComputedValue,
        attributes: &[KeyValue],
    ) {
        self.0.measure(value, attributes);
    }

    fn collect_data_points<DP, MapFn>(&self, dest: &mut Vec<DP>, mut map_fn: MapFn)
    where
        MapFn: FnMut(Vec<KeyValue>, &Self::Aggr) -> DP,
    {
        self.0
            .collect_and_reset(dest, |attributes, aggr| map_fn(attributes, &aggr));
    }
}

/// At the moment use [`ValueMap`] under the hood (which support both Delta and Cumulative), to implement `AggregateMap` for Cumulative temporality
/// Later this could be improved to support only Cumulative temporality
pub(crate) struct CumulativeValueMap<A>(ValueMap<A>)
where
    A: Aggregator;

impl<A> CumulativeValueMap<A>
where
    A: Aggregator,
{
    pub(crate) fn new(config: A::InitConfig) -> Self {
        Self(ValueMap::new(config))
    }
}

impl<A> AggregateMap for CumulativeValueMap<A>
where
    A: Aggregator,
    <A as Aggregator>::InitConfig: Send + Sync,
{
    const TEMPORALITY: Temporality = Temporality::Cumulative;

    type Aggr = A;

    fn measure(
        &self,
        value: <Self::Aggr as Aggregator>::PreComputedValue,
        attributes: &[KeyValue],
    ) {
        self.0.measure(value, attributes);
    }

    fn collect_data_points<DP, MapFn>(&self, dest: &mut Vec<DP>, map_fn: MapFn)
    where
        MapFn: FnMut(Vec<KeyValue>, &Self::Aggr) -> DP,
    {
        self.0.collect_readonly(dest, map_fn);
    }
}
