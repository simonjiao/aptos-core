// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

pub mod counters;
pub mod log_schema;

pub mod prelude {
    pub use crate::{
        alert, counters::CRITICAL_ERRORS, speculative_debug, speculative_error, speculative_info,
        speculative_log, speculative_trace, speculative_warn,
    };
}

use crate::{counters::CRITICAL_ERRORS, log_schema::AdapterLogSchema};
use aptos_logger::{prelude::*, Level};
use aptos_speculative_state_helper::{SpeculativeEvent, SpeculativeEvents};
use arc_swap::ArcSwapOption;
use once_cell::sync::Lazy;
use std::sync::Arc;

struct VMLogEntry {
    level: Level,
    context: AdapterLogSchema,
    message: String,
}

impl VMLogEntry {
    fn new(level: Level, context: AdapterLogSchema, message: String) -> Self {
        Self {
            level,
            context,
            message,
        }
    }
}

// Implement dispatching for a recorded VM log entry to support recording
// as speculative logging event (and eventual dispatching as needed).
impl SpeculativeEvent for VMLogEntry {
    fn dispatch(self) {
        match self.level {
            Level::Error => {
                // TODO: Consider using SpeculativeCounter to increase CRITICAL_ERRORS
                // on the critical path instead of async dispatching.
                alert!(self.context, "{}", self.message);
            },
            Level::Warn => warn!(self.context, "{}", self.message),
            Level::Info => info!(self.context, "{}", self.message),
            Level::Debug => debug!(self.context, "{}", self.message),
            Level::Trace => trace!(self.context, "{}", self.message),
        }
    }
}

static BUFFERED_LOG_EVENTS: Lazy<ArcSwapOption<SpeculativeEvents<VMLogEntry>>> =
    Lazy::new(|| ArcSwapOption::from(None));

/// Initializes the storage of speculative logs for num_txns many transactions.
pub fn init_speculative_logs(num_txns: usize) {
    BUFFERED_LOG_EVENTS.swap(Some(Arc::new(SpeculativeEvents::new(num_txns))));
}

/// Adds a message at a specified logging level and given context (that includes txn index)
/// to speculative buffer. Logs directly and logs a separate (new error) if the speculative
/// events storage is not initialized or appropriately sized.
pub fn speculative_log(level: Level, context: &AdapterLogSchema, message: String) {
    let txn_idx = context.get_txn_idx();
    match &*BUFFERED_LOG_EVENTS.load() {
        Some(log_events) => {
            let log_event = VMLogEntry::new(level, context.clone(), message);
            if let Err(e) = log_events.record(txn_idx, log_event) {
                alert!("{:?}", e);
            };
        },
        None => {},
    };
}

/// Flushes the first num_to_flush logs in the currently stored logs, and swaps the speculative log / event storage with None.
/// Must be called after block execution is complete (removes the storage from Arc).
pub fn flush_speculative_logs(num_to_flush: usize) {
    if let Some(log_events_ptr) = BUFFERED_LOG_EVENTS.swap(None) {
        match Arc::try_unwrap(log_events_ptr) {
            Ok(log_events) => log_events.flush(num_to_flush),
            Err(_) => {
                alert!("Speculative log storage must be uniquely owned to flush");
            },
        };
    }
}

/// Clear speculative logs recorded for a specific transction, useful when transaction
/// execution fails validation and aborts - setting stage for the re-execution.
pub fn clear_speculative_txn_logs(txn_idx: usize) {
    match &*BUFFERED_LOG_EVENTS.load() {
        Some(log_events) => {
            if let Err(e) = log_events.clear_txn_events(txn_idx) {
                alert!("{:?}", e);
            };
        },
        None => {},
    }
}

/// Combine logging and error and incrementing critical errors counter for alerting.
#[macro_export]
macro_rules! alert {
    ($($args:tt)+) => {
	error!($($args)+);
	CRITICAL_ERRORS.inc();
    };
}

#[macro_export]
macro_rules! speculative_error {
    ($($args:tt)+) => {
        if enabled!(Level::Error) {
            speculative_log(Level::Error, $($args)+);
        }
    };
}

#[macro_export]
macro_rules! speculative_warn {
    ($($args:tt)+) => {
        if enabled!(Level::Warn) {
            speculative_log(Level::Warn, $($args)+);
        }
    };
}

#[macro_export]
macro_rules! speculative_info {
    ($($args:tt)+) => {
        if enabled!(Level::Info) {
            speculative_log(Level::Info, $($args)+);
        }
    };
}

#[macro_export]
macro_rules! speculative_debug {
    ($($args:tt)+) => {
        if enabled!(Level::Debug) {
            speculative_log(Level::Debug, $($args)+);
        }
    };
}

#[macro_export]
macro_rules! speculative_trace {
    ($($args:tt)+) => {
        if enabled!(Level::Trace) {
            speculative_log(Level::Trace, $($args)+);
        }
    };
}
