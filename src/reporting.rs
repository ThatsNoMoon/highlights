// Copyright 2020 Benjamin Scherer
// Licensed under the Open Software License version 3.0

use once_cell::sync::OnceCell;
use reqwest::{
	blocking::{self, Client as BlockingClient},
	Client as AsyncClient,
};
use serde::Serialize;

use std::{panic, time::Duration};

use log::{Level, LevelFilter, Log, Metadata, Record};
use simplelog::{
	CombinedLogger, Config, ConfigBuilder, SharedLogger, TermLogger,
	TerminalMode,
};

use crate::{global::settings, Error};

static WEBHOOK_CLIENT: OnceCell<AsyncClient> = OnceCell::new();

#[derive(Serialize)]
struct WebhookMessage {
	content: String,
}

struct WebhookLogger;
impl Log for WebhookLogger {
	fn enabled(&self, meta: &Metadata) -> bool {
		meta.level() == Level::Error
	}

	fn log(&self, record: &Record) {
		if self.enabled(record.metadata()) {
			let content = format!("[{}] {}", record.level(), record.args());
			tokio::spawn(async move {
				if let Err(e) = report_error(content).await {
					log::warn!("Failed to report error: {}", e);
				}
			});
		}
	}

	fn flush(&self) {}
}
impl SharedLogger for WebhookLogger {
	fn level(&self) -> LevelFilter {
		LevelFilter::Error
	}

	fn config(&self) -> Option<&Config> {
		None
	}

	fn as_log(self: Box<Self>) -> Box<dyn Log> {
		Box::new(*self)
	}
}

pub fn init() {
	let mut loggers: Vec<Box<dyn SharedLogger>> = vec![];

	let mut default_config = ConfigBuilder::new();
	default_config.set_target_level(LevelFilter::Error);
	for (path, level) in &settings().logging.filters {
		default_config.add_filter_ignore(path.to_string());

		let mut config = ConfigBuilder::new();
		config.set_target_level(LevelFilter::Error);
		config.add_filter_allow(path.to_string());
		loggers.push(TermLogger::new(
			*level,
			config.build(),
			TerminalMode::Mixed,
		));
	}
	loggers.push(TermLogger::new(
		settings().logging.level,
		default_config.build(),
		TerminalMode::Mixed,
	));

	if settings().logging.webhook.is_some() {
		WEBHOOK_CLIENT
			.set(
				AsyncClient::builder()
					.build()
					.expect("Failed to build webhook client"),
			)
			.unwrap();

		let default_panic_hook = panic::take_hook();

		let reporting_panic_hook: Box<
			dyn Fn(&panic::PanicInfo<'_>) + Send + Sync + 'static,
		> = Box::new(move |info| {
			if let Err(e) = report_panic(info) {
				log::error!("Error reporting panic: {}", e);
			}

			default_panic_hook(info);
		});

		panic::set_hook(reporting_panic_hook);

		loggers.push(Box::new(WebhookLogger));
	} else {
		log::warn!("Webhook URL is not present, not reporting errors");
	}

	CombinedLogger::init(loggers).expect("Failed to set logger");
}

async fn report_error(content: String) -> Result<reqwest::Response, Error> {
	let url = settings()
		.logging
		.webhook
		.as_ref()
		.ok_or("Webhook URL not set")?
		.to_owned();
	let client = WEBHOOK_CLIENT.get().ok_or("Webhook client not set")?;

	let message = WebhookMessage { content };

	Ok(client
		.post(url)
		.json(&message)
		.timeout(Duration::from_secs(5))
		.send()
		.await?)
}

fn report_panic(info: &panic::PanicInfo) -> Result<blocking::Response, Error> {
	let url = settings()
		.logging
		.webhook
		.as_ref()
		.ok_or("Webhook URL not set")?
		.to_owned();
	let client = BlockingClient::builder().build()?;

	let message = WebhookMessage {
		content: format!("[PANIC] {}", info),
	};

	Ok(client
		.post(url)
		.json(&message)
		.timeout(Duration::from_secs(5))
		.send()?)
}
