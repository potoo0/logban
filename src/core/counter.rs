use std::time::Duration;

use time::OffsetDateTime;

/// A constant-memory, rate-estimation counter using a fail2ban-style approach.
///
/// Semantics:
/// - Maintains O(1) state: first and last timestamps, and estimated count
/// - Push events and query current estimated count
/// - Estimates the number of events in the current window using a simple linear rate
/// - Out-of-order events (earlier than last) are dropped
/// - Not a precise sliding window; suitable for burst detection with minimal memory
#[derive(Debug, Clone)]
pub struct RateEstimator {
    /// Duration of the window for `[first_time, last_time]`
    window: Duration,
    /// Timestamp of the first event in the current window
    first_time: Option<OffsetDateTime>,
    /// Timestamp of the last event
    last_time: Option<OffsetDateTime>,
    /// Estimated number of events in the current window
    estimated_count: u32,
}

impl RateEstimator {
    pub fn new(window: Duration) -> Self {
        Self { window, first_time: None, last_time: None, estimated_count: 0 }
    }

    /// Push a new event timestamp.
    /// Out-of-order events (earlier than last event) are ignored.
    pub fn push(&mut self, ts: OffsetDateTime) {
        match self.last_time {
            Some(last) if ts < last => {
                // drop out-of-order events
                return;
            }
            _ => {}
        }

        match self.first_time {
            None => {
                // first event
                self.first_time = Some(ts);
                self.last_time = Some(ts);
                self.estimated_count = 1;
            }
            Some(first) => {
                let window_secs = self.window.as_secs_f64();
                let elapsed = (ts - first).as_seconds_f64();

                if window_secs < elapsed {
                    // estimation from rate by previous known interval
                    let est =
                        ((self.estimated_count as f64) / elapsed * window_secs).round() as u32;
                    self.estimated_count = est + 1; // include current event
                    self.first_time = Some(ts - self.window);
                } else {
                    // Within window, increment
                    self.estimated_count += 1;
                }

                self.last_time = Some(ts);
            }
        }
    }

    /// Return the current estimated count in the window
    pub fn count(&self) -> u32 {
        self.estimated_count
    }

    /// Reset the counter
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.first_time = None;
        self.last_time = None;
        self.estimated_count = 0;
    }
}
