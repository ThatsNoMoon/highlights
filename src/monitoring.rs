// Copyright 2020 Benjamin Scherer
// Licensed under the Open Software License version 3.0

use hyper::{
	header::CONTENT_TYPE,
	service::{make_service_fn, service_fn},
	Body, Request, Response, Server,
};
use once_cell::sync::{Lazy, OnceCell};
use prometheus::{
	core::Collector, proto::MetricFamily, register_gauge_vec, Encoder,
	GaugeVec, TextEncoder,
};

use std::{net::SocketAddr, time::Instant};

use crate::global::settings;

static ENABLED: OnceCell<bool> = OnceCell::new();

static COMMAND_TIME_GAUGE: Lazy<GaugeVec, fn() -> GaugeVec> = Lazy::new(|| {
	register_gauge_vec!(
		concat!(env!("CARGO_PKG_NAME"), "_command_time"),
		"Command execution time, in seconds",
		&["name"]
	)
	.unwrap()
});

static QUERY_TIME_GAUGE: Lazy<GaugeVec, fn() -> GaugeVec> = Lazy::new(|| {
	register_gauge_vec!(
		concat!(env!("CARGO_PKG_NAME"), "_query_time"),
		"Query execution time, in seconds",
		&["name"]
	)
	.unwrap()
});

#[derive(Copy, Clone)]
enum TimerType {
	Command,
	Query,
}

pub struct Timer {
	kind: TimerType,
	name: &'static str,
	start: Instant,
}

impl Timer {
	pub fn command(name: &'static str) -> Self {
		Self {
			kind: TimerType::Command,
			name,
			start: Instant::now(),
		}
	}

	pub fn query(name: &'static str) -> Self {
		Self {
			kind: TimerType::Query,
			name,
			start: Instant::now(),
		}
	}
}

impl Drop for Timer {
	fn drop(&mut self) {
		if !ENABLED.get().unwrap() {
			return;
		}
		let elapsed = self.start.elapsed().as_secs_f64();

		match self.kind {
			TimerType::Command => {
				COMMAND_TIME_GAUGE
					.with_label_values(&[self.name])
					.set(elapsed);
			}
			TimerType::Query => {
				QUERY_TIME_GAUGE
					.with_label_values(&[self.name])
					.set(elapsed);
			}
		}
	}
}

pub fn avg_command_time() -> Option<f64> {
	avg_metrics(COMMAND_TIME_GAUGE.collect())
}

pub fn avg_query_time() -> Option<f64> {
	avg_metrics(QUERY_TIME_GAUGE.collect())
}

fn avg_metrics(metric_families: Vec<MetricFamily>) -> Option<f64> {
	let mut count = 0;
	let mut sum = 0.0;
	for metric_family in metric_families {
		for metric in metric_family.get_metric() {
			sum += metric.get_gauge().get_value();
			count += 1;
		}
	}

	if count == 0 {
		None
	} else {
		Some(sum / count as f64)
	}
}

pub fn init() {
	if let Some(addr) = settings().logging.prometheus {
		ENABLED.set(true).unwrap();
		tokio::spawn(prometheus_server(addr));
	} else {
		ENABLED.set(false).unwrap();

		log::warn!(
			"Prometheus address not provided; not starting monitoring server"
		);
	}
}

async fn prometheus_server(addr: SocketAddr) {
	let serve_future = Server::bind(&addr).serve(make_service_fn(|_| async {
		Ok::<_, hyper::Error>(service_fn(serve_req))
	}));

	if let Err(err) = serve_future.await {
		log::error!("Prometheus server error: {0}\n{0:?}", err);
	}
}

async fn serve_req(
	_req: Request<Body>,
) -> Result<Response<Body>, hyper::Error> {
	let encoder = TextEncoder::new();

	let metric_families = prometheus::gather();
	let mut buffer = vec![];
	encoder.encode(&metric_families, &mut buffer).unwrap();

	let response = Response::builder()
		.status(200)
		.header(CONTENT_TYPE, encoder.format_type())
		.body(Body::from(buffer))
		.unwrap();

	Ok(response)
}
