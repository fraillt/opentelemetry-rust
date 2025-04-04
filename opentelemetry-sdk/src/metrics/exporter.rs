//! Interfaces for exporting metrics

use opentelemetry::InstrumentationScope;

use crate::{error::OTelSdkResult, Resource};
use std::{fmt::Debug, time::Duration};

use super::{data::{Metric, ScopeMetrics}, Temporality};

/// A iterator over batch of aggregated metric to be exported by a [`PushMetricExporter`].
pub struct MetricBatch<'a> {
    iter: Box<dyn Iterator<Item = (&'a Metric, &'a InstrumentationScope)> + Send + 'a>
}

impl <'a> MetricBatch<'a> {
    pub(crate) fn new(batches: &'a [ScopeMetrics]) -> Self {
        let iter = batches.iter().flat_map(|batch| batch.metrics.iter().map(|metric| (metric, &batch.scope)));
        Self {
            iter: Box::new(iter)
        }
    }
}

impl<'a> Iterator for MetricBatch<'a> {
    type Item = (&'a Metric, &'a InstrumentationScope);
    
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

/// Exporter handles the delivery of metric data to external receivers.
///
/// This is the final component in the metric push pipeline.
pub trait PushMetricExporter: Send + Sync + Debug + 'static {
    /// Export serializes and transmits metric data to a receiver.
    ///
    /// All retry logic must be contained in this function. The SDK does not
    /// implement any retry logic. All errors returned by this function are
    /// considered unrecoverable and will be logged.
    fn export(
        &self,
        batch: MetricBatch<'_>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send;

    /// Flushes any metric data held by an exporter.
    fn force_flush(&self) -> OTelSdkResult;

    /// Releases any held computational resources.
    ///
    /// After Shutdown is called, calls to Export will perform no operation and
    /// instead will return an error indicating the shutdown state.
    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult;

    /// Shutdown with the default timeout of 5 seconds.
    fn shutdown(&self) -> OTelSdkResult {
        self.shutdown_with_timeout(Duration::from_secs(5))
    }

    /// Access the [Temporality] of the MetricExporter.
    fn temporality(&self) -> Temporality;

    /// Set the resource for the exporter.
    fn set_resource(&mut self, _resource: &Resource) {}
}
