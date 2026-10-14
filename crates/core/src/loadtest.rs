//! Load testing: send one request many times, concurrently, and measure.
//!
//! Individual requests are not reported as separate spans (thousands per
//! second would drown Live Activity). Instead the load test span shows live
//! progress and a statistics summary every second.

use crate::engine::{Engine, SendOptions};
use crate::error::Result;
use crate::events::{EventBus, BusConfig, Level, Outcome, Span, SpanId, SpanKind, Source, Summary};
use crate::model::Request;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct LoadOptions {
    pub source: Source,
    pub environment: Option<String>,
    /// Requests in flight at the same time.
    pub concurrency: u32,
    /// Total number of requests.
    pub requests: u32,
}

/// Results of a load test. Times are in milliseconds.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LoadReport {
    pub span_id: Option<SpanId>,
    pub requests: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub duration_ms: f64,
    pub requests_per_second: f64,
    pub min_ms: f64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    /// Status code → count.
    pub status_codes: std::collections::BTreeMap<u16, u64>,
}

impl LoadReport {
    fn from_samples(samples: &mut [f64], statuses: &[u16], failed: u64, elapsed: Duration) -> Self {
        samples.sort_by(f64::total_cmp);
        let percentile = |p: f64| {
            if samples.is_empty() {
                0.0
            } else {
                samples[((samples.len() - 1) as f64 * p).round() as usize]
            }
        };
        let mut status_codes = std::collections::BTreeMap::new();
        for status in statuses {
            *status_codes.entry(*status).or_insert(0) += 1;
        }
        let requests = samples.len() as u64 + failed;
        let seconds = elapsed.as_secs_f64().max(f64::EPSILON);
        Self {
            span_id: None,
            requests,
            succeeded: samples.len() as u64,
            failed,
            duration_ms: crate::events::millis(elapsed),
            requests_per_second: requests as f64 / seconds,
            min_ms: samples.first().copied().unwrap_or_default(),
            mean_ms: if samples.is_empty() { 0.0 } else { samples.iter().sum::<f64>() / samples.len() as f64 },
            p50_ms: percentile(0.50),
            p90_ms: percentile(0.90),
            p99_ms: percentile(0.99),
            max_ms: samples.last().copied().unwrap_or_default(),
            status_codes,
        }
    }

    /// One line, e.g. "120 req · 98.2 req/s · p50 12.0 ms · p99 40.1 ms · 0 failed".
    pub fn summary_line(&self) -> String {
        format!(
            "{} req · {:.1} req/s · p50 {:.1} ms · p90 {:.1} ms · p99 {:.1} ms · {} failed",
            self.requests, self.requests_per_second, self.p50_ms, self.p90_ms, self.p99_ms, self.failed
        )
    }
}

#[derive(Default)]
struct Samples {
    times: Vec<f64>,
    statuses: Vec<u16>,
}

impl Engine {
    /// Sends `request` `options.requests` times with `options.concurrency` workers.
    pub async fn load_test(self: &Arc<Self>, request: &Request, request_id: Option<String>, options: LoadOptions) -> Result<LoadReport> {
        let title = format!("Load test: {} {}", request.method.as_str(), if request.name.is_empty() { &request.url } else { &request.name });
        let span = Span::start(
            self.bus(),
            options.source.clone(),
            None,
            SpanKind::Request,
            title,
            Summary { protocol: Some("load".into()), method: Some(request.method.as_str().into()), url: Some(request.url.clone()), request_id: request_id.clone() },
        );
        let guard = self.track(&span, None);
        let total = options.requests.max(1);
        let concurrency = options.concurrency.clamp(1, 1_000).min(total);
        span.log(Level::Info, format!("{total} requests, {concurrency} at a time"));

        // Requests run against a private, silent bus so they don't flood Live Activity.
        let quiet = Arc::new(Engine::new(EventBus::new(BusConfig { history_len: 1, blob_capacity_bytes: 0 }), self.workspace(), self.secrets().clone()));
        quiet.set_extensions(self.extensions());
        for (k, v) in self.runtime_variables() {
            quiet.set_runtime_variable(&k, &v);
        }

        let next = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicU64::new(0));
        let samples = Arc::new(Mutex::new(Samples::default()));
        let started = Instant::now();
        let mut workers = Vec::new();
        for _ in 0..concurrency {
            let (quiet, next, failed, samples, request, cancel) =
                (quiet.clone(), next.clone(), failed.clone(), samples.clone(), request.clone(), guard.token.clone());
            let (environment, request_id) = (options.environment.clone(), request_id.clone());
            workers.push(tokio::spawn(async move {
                while next.fetch_add(1, Ordering::Relaxed) < u64::from(total) && !cancel.is_cancelled() {
                    let mut send = SendOptions::new(Source::System).environment(environment.clone());
                    send.request_id = request_id.clone();
                    let execution = quiet.send(&request, send).await;
                    match &execution.response {
                        Some(response) if execution.error.is_none() => {
                            let mut s = samples.lock().await;
                            s.times.push(crate::events::millis(response.total));
                            s.statuses.push(response.status);
                        }
                        _ => {
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }));
        }

        // Report progress every second until all workers finish.
        let reporter = {
            let (span, samples, failed) = (span.clone(), samples.clone(), failed.clone());
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let s = samples.lock().await;
                    let done = s.times.len() as u64 + failed.load(Ordering::Relaxed);
                    let mut times = s.times.clone();
                    let report = LoadReport::from_samples(&mut times, &s.statuses, failed.load(Ordering::Relaxed), started.elapsed());
                    drop(s);
                    span.progress(u32::try_from(done).unwrap_or(u32::MAX), total);
                    span.log(Level::Info, report.summary_line());
                }
            })
        };
        for worker in workers {
            let _ = worker.await;
        }
        reporter.abort();

        let s = samples.lock().await;
        let mut times = s.times.clone();
        let mut report = LoadReport::from_samples(&mut times, &s.statuses, failed.load(Ordering::Relaxed), started.elapsed());
        report.span_id = Some(span.id());
        span.progress(u32::try_from(report.requests).unwrap_or(u32::MAX), total);
        span.log(Level::Info, format!("Finished: {}", report.summary_line()));
        let outcome = if guard.token.is_cancelled() {
            Outcome::Cancelled
        } else if report.failed > 0 {
            Outcome::Failed
        } else {
            Outcome::Ok
        };
        span.finish(outcome, None, None, None);
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles() {
        let mut samples: Vec<f64> = (1..=100).map(f64::from).collect();
        let report = LoadReport::from_samples(&mut samples, &[200; 100], 0, Duration::from_secs(1));
        assert_eq!(report.p50_ms, 51.0);
        assert_eq!(report.p99_ms, 99.0);
        assert_eq!(report.requests_per_second, 100.0);
    }
}
